// SPDX-License-Identifier: Apache-2.0
//! A formula as `FreeCAD`'s `ExpressionParser` reads it, and its value as
//! `FreeCAD`'s evaluator computes it: the classes of value the result can
//! have, by the rules of its operators and functions over the classes of the
//! properties it references.

use std::rc::Rc;

use cadmpeg_ir::design::units::Quantity;

use super::{unit_word, Dimensions, Unit, UnitState};

mod formatting;
mod identifier;

/// The most tokens of a formula the reader parses.
const MAX_TOKENS: usize = 1024;

/// The deepest nesting of parentheses, calls, and signs the reader parses.
const MAX_NESTING: usize = 64;

/// The amount by which the exponent of `pow` with a unit may exceed its
/// rounded value (`FunctionExpression::evaluate`, `POW`).
const INTEGRAL: f64 = 1e-9;

/// The distance from an integer within which `Unit::pow` takes the product
/// of a unit's exponent and a power as that integer.
const UNIT_POWER: f64 = f64::EPSILON;

/// The exclusive upper bound and the magnitude of the inclusive lower
/// bound of a unit exponent (`Unit::checkRange`).
const UNIT_EXPONENT_LIMIT: i8 = 8;

/// The magnitude below which an integral number is a Python `int`
/// (`essentiallyInteger`, the range of a C `long`).
const WHOLE: f64 = 9.223_372_036_854_776e18;

/// `FreeCAD` narrows a positive whole value through C int in this interval.
/// The result of the out-of-range cast depends on the producer build.
pub(in crate::design) fn producer_number(number: f64) -> Option<f64> {
    let ambiguous = number.fract() == 0.0 && number > f64::from(i32::MAX) && number <= WHOLE;
    (!ambiguous).then_some(number)
}

/// A formula the reader does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Unparsed {
    /// `FreeCAD`'s parser also refuses the text.
    Syntax,
    /// The text is larger or deeper than the reader parses.
    Limit,
}

/// The kind of a formula's result (§5.12, Kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Kind {
    Quantity(Quantity),
    /// A Python `bool`.
    Boolean,
    Text,
    /// A kind that the file does not determine.
    Unknown,
}

/// A class of value that `FreeCAD`'s evaluator tells apart. `calc` admits
/// every class but `Refused`; `pyToQuantity` takes only `Quantity`, `Int`,
/// `Float`, and `Boolean`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Class {
    /// A `QuantityPy`, with its quantity where the model knows its unit.
    Quantity(Option<Dimensions>),
    /// A Python `int`.
    Int,
    /// A Python `float`.
    Float,
    /// A Python `bool`.
    Boolean,
    /// A `str`.
    Text,
    /// A `VectorPy`.
    Vector,
    Placement,
    List,
    Dict,
    Tuple,
    /// Another object that `calc` admits.
    Object,
    /// A value that `calc` refuses: `None`, a `tuple`, a `set`, or an
    /// instance of a Python class.
    Refused,
    /// An evaluation that raises.
    Raises,
}

use Class::{
    Boolean, Dict, Float, Int, List, Object, Placement, Raises, Refused, Text, Tuple, Vector,
};

impl Class {
    pub(in crate::design) const fn quantity(quantity: Quantity) -> Self {
        Self::Quantity(Some(Dimensions::from_quantity(quantity)))
    }
}

/// Every class: the value of a reference the model does not resolve.
const ANY: &[Class] = &[
    Class::Quantity(None),
    Int,
    Float,
    Boolean,
    Text,
    Vector,
    Placement,
    List,
    Dict,
    Tuple,
    Object,
    Refused,
    Raises,
];

/// The classes a value can have and a fact shared by every possibility.
#[derive(Debug, Clone, PartialEq, Default)]
pub(in crate::design) struct Value(Vec<Class>, Option<Fact>);

#[derive(Debug, Clone, PartialEq)]
enum Fact {
    Number(f64),
    WideInteger(Rc<str>),
    Text(Rc<str>),
    TextBound(usize),
    None,
    Items(Rc<[Value]>),
}

impl Value {
    pub(in crate::design) fn of(classes: &[Class]) -> Self {
        let mut value = Self::default();
        for class in classes {
            value.insert(*class);
        }
        value
    }

    pub(in crate::design) fn any() -> Self {
        Self::of(ANY)
    }

    pub(in crate::design) fn numeric(class: Class, number: Option<f64>) -> Self {
        Self::of(&[class]).with_number(number)
    }

    pub(in crate::design) fn integer_token(text: &str) -> Self {
        if let Ok(number) = text.parse::<i32>() {
            return Self::numeric(Int, Some(f64::from(number)));
        }
        if text.len() <= MAX_TRACKED_TEXT {
            return Self(vec![Int], Some(Fact::WideInteger(text.into())));
        }
        Self::of(&[Int, Refused])
    }

    pub(in crate::design) fn text(text: impl Into<Rc<str>>) -> Self {
        Self(vec![Text], Some(Fact::Text(text.into())))
    }

    pub(in crate::design) fn none() -> Self {
        Self(vec![Refused], Some(Fact::None))
    }

    fn items(class: Class, mut items: Vec<Self>) -> Self {
        for item in &mut items {
            if matches!(item.1, Some(Fact::Items(_))) {
                item.1 = None;
            }
        }
        Self(vec![class], Some(Fact::Items(items.into())))
    }

    fn with_number(mut self, number: Option<f64>) -> Self {
        self.1 = number
            .filter(|number| {
                !self.0.is_empty()
                    && self.0.iter().all(|class| is_number(*class))
                    && (!self.contains(Int)
                        || (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(number))
            })
            .map(Fact::Number);
        self
    }

    pub(in crate::design) fn numeric_fact(&self) -> Option<f64> {
        match self.1 {
            Some(Fact::Number(number)) => Some(number),
            _ => None,
        }
    }

    fn text_fact(&self) -> Option<&str> {
        match &self.1 {
            Some(Fact::Text(text)) => Some(text),
            _ => None,
        }
    }

    fn item_facts(&self) -> Option<&[Self]> {
        match &self.1 {
            Some(Fact::Items(items)) => Some(items),
            _ => None,
        }
    }

    fn truth(&self) -> Option<bool> {
        match &self.1 {
            Some(Fact::Number(number)) => Some(*number != 0.0),
            Some(Fact::Text(text)) => Some(!text.is_empty()),
            Some(Fact::None) => Some(false),
            Some(Fact::Items(items)) => Some(!items.is_empty()),
            Some(Fact::WideInteger(_) | Fact::TextBound(_)) | None => None,
        }
    }

    /// The plain number `value`: an `int` where it is whole (`pyFromQuantity`,
    /// `essentiallyInteger`).
    pub(in crate::design) fn number(value: f64) -> Class {
        let limit = if std::ffi::c_long::BITS == 32 {
            f64::from(i32::MAX) + 1.0
        } else {
            WHOLE
        };
        if value.fract() == 0.0 && (-limit..limit).contains(&value) {
            Int
        } else {
            Float
        }
    }

    /// The value holds every class.
    pub(in crate::design) fn is_any(&self) -> bool {
        ANY.iter().all(|class| self.contains(*class))
    }

    pub(in crate::design) fn classes(&self) -> &[Class] {
        &self.0
    }

    pub(in crate::design) fn insert(&mut self, class: Class) {
        if !self.0.contains(&class) {
            self.1 = None;
            self.0.push(class);
        }
    }

    fn join(&mut self, other: Self) {
        if self.0.is_empty() {
            *self = other;
            return;
        }
        if other.0.is_empty() {
            return;
        }
        let fact = (self.1 == other.1).then(|| self.1.take()).flatten();
        for class in other.0 {
            self.insert(class);
        }
        self.1 = fact;
    }

    fn contains(&self, class: Class) -> bool {
        self.0.contains(&class)
    }

    /// The kind of the value: the quantity that every class shares, where
    /// each is a number of a known unit, and a bool or a text where that is
    /// the one class.
    pub(in crate::design) fn kind(&self) -> Kind {
        let mut found = None;
        for class in &self.0 {
            let quantity = match class {
                Class::Quantity(Some(dimensions)) => match dimensions.quantity() {
                    Some(quantity) => quantity,
                    None => return Kind::Unknown,
                },
                Int | Float => Quantity::REAL,
                Boolean if self.0.len() == 1 => return Kind::Boolean,
                Text if self.0.len() == 1 => return Kind::Text,
                Class::Quantity(None)
                | Boolean
                | Text
                | Vector
                | Placement
                | List
                | Dict
                | Tuple
                | Object
                | Refused
                | Raises => return Kind::Unknown,
            };
            if found.is_some_and(|found| found != quantity) {
                return Kind::Unknown;
            }
            found = Some(quantity);
        }
        found.map_or(Kind::Unknown, Kind::Quantity)
    }

    pub(in crate::design) fn into_stored(self) -> Self {
        let number = self.numeric_fact();
        let mut stored = Self::default();
        for class in &self.0 {
            match class {
                Class::Quantity(Some(Dimensions::REAL)) | Float => match number {
                    Some(number) => stored.insert(Self::number(number)),
                    None => stored.join(Self::of(&[Int, Float])),
                },
                Class::Quantity(None) => {
                    stored.join(Self::of(&[Class::Quantity(None), Int, Float]));
                }
                Raises => stored.insert(Text),
                Class::Quantity(Some(_))
                | Int
                | Boolean
                | Text
                | Vector
                | Placement
                | List
                | Dict
                | Tuple
                | Object
                | Refused => stored.insert(*class),
            }
        }
        if number.is_some() {
            return stored.with_number(number);
        }
        if !self.contains(Raises) && !matches!(self.1, Some(Fact::WideInteger(_))) {
            stored.1 = self.1;
        }
        stored
    }
}

/// A node of a parsed formula.
#[derive(Debug)]
pub(in crate::design) enum Node<'a> {
    /// A number as written, with the unit that follows it.
    Number {
        text: &'a str,
        number: f64,
        unit: Option<Unit>,
    },
    /// The divisor of `x / unit`.
    Unit(Unit),
    /// `pi`, `e`, `True`, `False`, `true`, `false`, or `None`.
    Constant {
        name: &'a str,
        value: f64,
        unit: Option<Unit>,
    },
    /// A `<<…>>` string.
    Text(&'a str),
    Path(Path),
    Indexed(Path, Vec<Node<'a>>),
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
        function: Function,
        args: Vec<Argument<'a>>,
    },
    Aggregate {
        collector: Collector,
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
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

/// An argument of a call: a value, or a range of cells `from:to`.
#[derive(Debug)]
pub(in crate::design) enum Argument<'a> {
    Value(Node<'a>),
    Range(CellRange<'a>),
}

/// The endpoint names and token spans of a range argument.
#[derive(Debug)]
pub(in crate::design) struct CellRange<'a> {
    pub(in crate::design) from: &'a str,
    pub(in crate::design) to: &'a str,
    pub(in crate::design) spans: [std::ops::Range<usize>; 2],
}

/// A reference to a property, or to a cell of a sheet.
#[derive(Debug, Clone)]
pub(in crate::design) struct Path {
    /// The byte range of the path in the formula.
    pub(in crate::design) span: std::ops::Range<usize>,
    pub(in crate::design) origin: Origin,
    pub(in crate::design) components: Vec<String>,
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
    Range(&'n CellRange<'a>),
}

/// What the references of a formula name.
pub(in crate::design) trait Scope {
    /// The value of the property or cell that `path` names.
    fn path(&mut self, path: &Path) -> Value;
    /// The values of the used cells from `from` to `to` of the formula's
    /// sheet, where the range resolves.
    fn range(&mut self, from: &str, to: &str) -> Option<Vec<Value>>;
}

/// A parsed expression and any references recovered when a reader limit intervenes.
pub(in crate::design) struct Parsed<'a> {
    pub(in crate::design) tree: Result<Node<'a>, Unparsed>,
    pub(in crate::design) partial_paths: Vec<Path>,
}

/// Parse a formula without its leading equals mark.
pub(in crate::design) fn parse(text: &str) -> Result<Node<'_>, Unparsed> {
    parse_with_paths(text).tree
}

pub(in crate::design) fn parse_with_paths(text: &str) -> Parsed<'_> {
    let (mut parser, incomplete) = parser(text);
    let tree = incomplete
        .and_then(|()| parser.expression())
        .and_then(|node| {
            if parser.done() {
                Ok(node)
            } else {
                Err(Unparsed::Syntax)
            }
        });
    let partial_paths = if matches!(tree, Err(Unparsed::Limit)) {
        parser.partial_paths()
    } else {
        Vec::new()
    };
    Parsed {
        tree,
        partial_paths,
    }
}

pub(super) fn parse_unit(text: &str) -> Option<Unit> {
    let (mut parser, complete) = parser(text);
    complete.ok()?;
    let unit = parser.unit_expression().ok()?;
    parser.done().then_some(unit)
}

fn parser(text: &str) -> (Parser<'_>, Result<(), Unparsed>) {
    let (lexemes, result) = lex(text);
    (
        Parser {
            lexemes,
            at: 0,
            nesting: 0,
        },
        result,
    )
}

impl<'a> Node<'a> {
    /// The kind of the node's result.
    pub(in crate::design) fn kind(&self, scope: &mut dyn Scope) -> Kind {
        self.value(scope).kind()
    }

    /// The classes of the node's result.
    pub(in crate::design) fn value(&self, scope: &mut dyn Scope) -> Value {
        match self {
            Node::Number { number, unit, .. } => match unit {
                Some(unit) => literal(unit, producer_number(*number)),
                None => producer_number(*number).map_or_else(
                    || Value::of(&[Int, Float]),
                    |number| Value::numeric(Value::number(number), Some(number)),
                ),
            },
            Node::Unit(unit) => literal(unit, Some(1.0)),
            Node::Constant {
                name: "None",
                unit: Some(_),
                ..
            } => Value::of(&[Raises]),
            Node::Constant {
                unit: Some(unit),
                value,
                ..
            } => literal(unit, Some(*value)),
            Node::Constant { name: "None", .. } => Value::none(),
            Node::Constant { name, value, .. } => Value::numeric(
                match *name {
                    "True" | "False" | "true" | "false" => Boolean,
                    _ => Float,
                },
                Some(*value),
            ),
            Node::Text(text) => Value::text(unquote(text)),
            Node::Path(path) => scope.path(path),
            Node::Indexed(..) => Value::any(),
            Node::Negate(operand) | Node::Plus(operand) => {
                let operand = operand.value(scope);
                let mut value = Value::default();
                for class in operand.0.iter().copied() {
                    value.join(signed(class));
                }
                let negative = matches!(self, Node::Negate(_));
                value.with_number(
                    operand
                        .numeric_fact()
                        .map(|n| if negative { -n } else { n }),
                )
            }
            Node::Group(inner) => inner.value(scope),
            Node::Binary { op, left, right } => {
                let (left, right) = (left.value(scope), right.value(scope));
                binary(*op, &left, &right)
            }
            Node::Conditional {
                condition,
                then,
                otherwise,
            } => {
                let condition = condition.value(scope);
                if condition.classes() == [Raises] {
                    return condition;
                }
                if let Some(taken) = condition.truth() {
                    return if taken { then } else { otherwise }.value(scope);
                }
                let raises = condition.contains(Raises);
                let mut value = then.value(scope);
                value.join(otherwise.value(scope));
                if raises {
                    value.insert(Raises);
                }
                value
            }
            Node::Call { function, args } => call(*function, args, scope),
            Node::Aggregate { collector, args } => aggregate(*collector, args, scope),
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
            Node::Indexed(path, arguments) => {
                found.push(Reference::Path(path));
                for argument in arguments {
                    argument.collect(found);
                }
            }
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
            Node::Call { args, .. } | Node::Aggregate { args, .. } => {
                for arg in args {
                    match arg {
                        Argument::Value(node) => node.collect(found),
                        Argument::Range(range) => found.push(Reference::Range(range)),
                    }
                }
            }
            Node::Number { .. } | Node::Unit(_) | Node::Constant { .. } | Node::Text(_) => {}
        }
    }
}

fn unquote(text: &str) -> Rc<str> {
    let text = &text[2..text.len() - 2];
    if !text.contains('\\') {
        return Rc::from(text);
    }
    let mut unquoted = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.next() {
                Some('t') => unquoted.push('\t'),
                Some('n') => unquoted.push('\n'),
                Some('r') => unquoted.push('\r'),
                Some(escaped @ ('\\' | '\'' | '"')) => unquoted.push(escaped),
                _ => {}
            },
            ch => unquoted.push(ch),
        }
    }
    Rc::from(unquoted)
}

impl Dimensions {
    const LENGTH: Self = Self::from_quantity(Quantity::LENGTH);
    const ANGLE: Self = Self::from_quantity(Quantity::ANGLE);

    fn zip(self, other: Self, operation: impl Fn(i8, i8) -> Option<i8>) -> Option<Self> {
        let mut result = self.0;
        for (slot, exponent) in result.iter_mut().zip(other.0) {
            *slot = operation(*slot, exponent)?;
        }
        Some(Self(result))
    }

    fn map(self, operation: impl Fn(i8) -> Option<i8>) -> Option<Self> {
        self.zip(Self::REAL, |exponent, _| operation(exponent))
    }

    pub(super) fn in_range(self) -> bool {
        self.0
            .into_iter()
            .all(|power| exponent(i32::from(power)).is_some())
    }
}

/// A number with a unit, or a unit alone.
fn literal(unit: &Unit, number: Option<f64>) -> Value {
    match unit.state {
        UnitState::Known => {
            let number = number.map(|number| number * unit.freecad_scale());
            Value::numeric(Class::Quantity(Some(unit.dimensions())), number)
        }
        UnitState::Error => Value::of(&[Raises]),
        UnitState::Unmodeled => Value::any(),
    }
}

impl Operator {
    fn compare(self, left: f64, right: f64) -> Option<bool> {
        Some(match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::Less => left < right,
            Self::LessEqual => left <= right,
            Self::Greater => left > right,
            Self::GreaterEqual => left >= right,
            Self::Add
            | Self::Subtract
            | Self::Multiply
            | Self::Divide
            | Self::Remainder
            | Self::Power => return None,
        })
    }

    fn fold(self, left: &Value, right: &Value, classes: &[Class]) -> Option<f64> {
        if self == Self::Subtract
            && matches!(
                (&left.1, &right.1),
                (Some(Fact::WideInteger(left)), Some(Fact::WideInteger(right))) if left == right
            )
        {
            return Some(0.0);
        }
        if let [Boolean] = classes {
            if let (Some(left), Some(right)) = (left.text_fact(), right.text_fact()) {
                let order = left.cmp(right);
                let ordered = match order {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                };
                return self
                    .compare(ordered, 0.0)
                    .map(|result| f64::from(u8::from(result)));
            }
            if left
                .0
                .iter()
                .chain(&right.0)
                .any(|class| matches!(class, Class::Quantity(_)))
            {
                return None;
            }
            let (left, right) = (left.numeric_fact()?, right.numeric_fact()?);
            if left.is_nan() || right.is_nan() {
                return None;
            }
            return self
                .compare(left, right)
                .map(|result| f64::from(u8::from(result)));
        }
        let (left, right) = (left.numeric_fact()?, right.numeric_fact()?);
        Some(match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide => left / right,
            // The exact integer result and Python's floating power errors
            // are separate from libm's approximation.
            Self::Power if classes.contains(&Int) && (0.0..=53.0).contains(&right) => {
                let right = (0..=53).find(|power| f64::from(*power) == right)?;
                left.powi(right)
            }
            _ => return None,
        })
    }
}

/// The unit of a number class, where the model knows it: a plain number is
/// dimensionless.
fn unit(class: Class) -> Option<Dimensions> {
    match class {
        Class::Quantity(quantity) => quantity,
        Int | Float | Boolean => Some(Dimensions::REAL),
        Text | Vector | Placement | List | Dict | Tuple | Object | Refused | Raises => None,
    }
}

/// The class is a number that `pyToQuantity` takes.
const fn is_number(class: Class) -> bool {
    matches!(class, Class::Quantity(_) | Int | Float | Boolean)
}

/// `-x` and `+x` (`calc`, `PyNumber_Negative`): a bool is an `int`.
fn signed(class: Class) -> Value {
    match class {
        Class::Quantity(quantity) => Value::of(&[Class::Quantity(quantity)]),
        Int | Boolean => Value::of(&[Int]),
        Float => Value::of(&[Float]),
        Text | List | Dict | Tuple | Refused | Raises => Value::of(&[Raises]),
        Vector => Value::of(&[Vector]),
        Placement | Object => Value::any(),
    }
}

/// `left op right`: join the results of every pair of operand classes.
fn binary(op: Operator, left: &Value, right: &Value) -> Value {
    let mut value = Value::default();
    for lhs in &left.0 {
        for rhs in &right.0 {
            let result = match (op, *lhs, *rhs) {
                (Operator::Remainder, Text, class) if class != Raises => {
                    formatting::remainder(left.text_fact(), right, class)
                }
                (Operator::Multiply, sequence @ (Text | List), count @ (Int | Boolean)) => {
                    repeat(sequence, left, count, right)
                }
                (Operator::Multiply, count @ (Int | Boolean), sequence @ (Text | List)) => {
                    repeat(sequence, right, count, left)
                }
                _ => pair(op, *lhs, *rhs, left.numeric_fact(), right.numeric_fact()),
            };
            value.join(result);
        }
    }
    if value.0 == [Text] && op == Operator::Add {
        if let (Some(left), Some(right)) = (left.text_fact(), right.text_fact()) {
            if left
                .len()
                .checked_add(right.len())
                .is_some_and(|length| length <= MAX_TRACKED_TEXT)
            {
                return Value::text(format!("{left}{right}"));
            }
        }
    }
    if let Some(number) = op.fold(left, right, &value.0) {
        return value.with_number(Some(number));
    }
    value
}

const MAX_TRACKED_TEXT: usize = 4096;

fn repeat(sequence: Class, items: &Value, count: Class, multiplier: &Value) -> Value {
    let proved = count == Boolean
        || multiplier.numeric_fact().is_some_and(|number| {
            if number <= 1.0 {
                return true;
            }
            let size = match sequence {
                Text => match &items.1 {
                    Some(Fact::TextBound(bound)) => Some(*bound),
                    _ => items.text_fact().map(str::len),
                },
                List => items.item_facts().map(<[Value]>::len),
                _ => None,
            };
            size.and_then(|len| u32::try_from(len).ok())
                .zip(u32::try_from(MAX_TRACKED_TEXT).ok())
                .is_some_and(|(len, bound)| f64::from(len) * number <= f64::from(bound))
        });
    outcomes(sequence, proved.then_some(true))
}

/// `left op right` for one class of each operand (`calc`): `calc` refuses a
/// `Refused` operand, except the right operand of a string's `%`, and an
/// operand that raises raises.
fn pair(op: Operator, left: Class, right: Class, lhs: Option<f64>, rhs: Option<f64>) -> Value {
    match op {
        Operator::Equal | Operator::NotEqual => equal(left, right),
        Operator::Less | Operator::LessEqual | Operator::Greater | Operator::GreaterEqual => {
            order(left, right)
        }
        Operator::Add | Operator::Subtract | Operator::Multiply | Operator::Divide => {
            arithmetic(op, left, right, rhs)
        }
        Operator::Remainder => remainder(left, right, rhs),
        Operator::Power => power(left, right, lhs, rhs),
    }
}

/// Equality for operand classes with a known rich-comparison protocol.
fn equal(left: Class, right: Class) -> Value {
    match (left, right) {
        (Refused | Raises | Tuple, _) | (_, Refused | Raises | Tuple) => Value::of(&[Raises]),
        (Object, _)
        | (_, Object)
        | (List, List)
        | (Dict, Dict)
        | (Class::Quantity(_), Vector | Placement)
        | (Vector | Placement, Class::Quantity(_)) => Value::of(&[Boolean, Raises]),
        (
            Class::Quantity(_) | Int | Float | Boolean | Text | Vector | Placement | List | Dict,
            Class::Quantity(_) | Int | Float | Boolean | Text | Vector | Placement | List | Dict,
        ) => Value::of(&[Boolean]),
    }
}

/// `<`, `<=`, `>`, and `>=`: two quantity objects need one unit
/// (`Quantity::operator<`), a quantity object and a plain number compare
/// their values (`QuantityPy::richCompare`), and Python orders two numbers
/// or two strings.
fn order(left: Class, right: Class) -> Value {
    match (left, right) {
        (Refused | Raises | Tuple, _) | (_, Refused | Raises | Tuple) => Value::of(&[Raises]),
        (Class::Quantity(Some(left)), Class::Quantity(Some(right))) => {
            Value::of(&[if left == right { Boolean } else { Raises }])
        }
        (Class::Quantity(_), Class::Quantity(_)) => Value::of(&[Boolean, Raises]),
        (
            Class::Quantity(_) | Int | Float | Boolean,
            Class::Quantity(_) | Int | Float | Boolean,
        )
        | (Text, Text) => Value::of(&[Boolean]),
        (Object, _) | (_, Object) | (List, List) => Value::of(&[Boolean, Raises]),
        _ => Value::of(&[Raises]),
    }
}

/// `+`, `-`, `*`, and `/`: strings concatenate and repeat by an `int`; a
/// quantity object takes a plain number as dimensionless, and `+` and `-`
/// need one unit (`QuantityPy` number handlers, `Quantity::operator+`).
fn arithmetic(op: Operator, left: Class, right: Class, rhs: Option<f64>) -> Value {
    match (left, right) {
        (Refused | Raises | Tuple, _) | (_, Refused | Raises | Tuple) => Value::of(&[Raises]),
        (Vector, Vector) => Value::of(&[match op {
            Operator::Add | Operator::Subtract => Vector,
            Operator::Multiply => Float,
            _ => Raises,
        }]),
        (Vector, Int | Float | Boolean | Class::Quantity(_)) if op == Operator::Divide => {
            nonzero_divisor(rhs, Vector)
        }
        (Vector, Int | Float | Boolean | Class::Quantity(_)) | (Int | Float | Boolean, Vector)
            if op == Operator::Multiply =>
        {
            Value::of(&[Vector])
        }
        (List, List) if op == Operator::Add => Value::of(&[List]),
        (Text, Text) => Value::of(&[if op == Operator::Add { Text } else { Raises }]),
        (Text, Class::Quantity(_) | Int | Float | Boolean)
        | (Class::Quantity(_) | Int | Float | Boolean, Text) => Value::of(&[Raises]),
        (Int | Boolean, Int | Boolean) if op != Operator::Divide => Value::of(&[Int]),
        (Int | Float | Boolean, Int | Float | Boolean) if op == Operator::Divide => {
            nonzero_divisor(rhs, Float)
        }
        (Int | Float | Boolean, Int | Float | Boolean) => Value::of(&[Float]),
        (Class::Quantity(_), Class::Quantity(_) | Int | Float | Boolean)
        | (Int | Float | Boolean, Class::Quantity(_)) => match op {
            Operator::Multiply => product(unit(left), unit(right), 1),
            Operator::Divide => product(unit(left), unit(right), -1),
            _ => match (unit(left), unit(right)) {
                (Some(left), Some(right)) if left == right => {
                    Value::of(&[Class::Quantity(Some(left))])
                }
                (Some(_), Some(_)) => Value::of(&[Raises]),
                (Some(known), None) | (None, Some(known)) => {
                    Value::of(&[Class::Quantity(Some(known)), Raises])
                }
                (None, None) => Value::of(&[Class::Quantity(None), Raises]),
            },
        },
        (Object | Placement, _) | (_, Object | Placement) | (Class::Quantity(_), Vector) => {
            Value::any()
        }
        _ => Value::of(&[Raises]),
    }
}

fn nonzero_divisor(number: Option<f64>, result: Class) -> Value {
    match number {
        Some(0.0) => Value::of(&[Raises]),
        Some(_) => Value::of(&[result]),
        None => Value::of(&[result, Raises]),
    }
}

/// A quantity object of the product of `left` and `right` to the power
/// `sign`, 1 or -1.
fn product(left: Option<Dimensions>, right: Option<Dimensions>, sign: i8) -> Value {
    match (left, right) {
        (Some(left), Some(right)) => {
            Value::of(&[combine(left, right, sign)
                .map_or(Raises, |quantity| Class::Quantity(Some(quantity)))])
        }
        _ => Value::of(&[Class::Quantity(None), Raises]),
    }
}

/// `%`: a string formats its right operand, a quantity object keeps its unit
/// and takes a quantity or a plain number, and a plain number's remainder by
/// a quantity object raises (`QuantityPy::number_remainder_handler`).
fn remainder(left: Class, right: Class, rhs: Option<f64>) -> Value {
    match (left, right) {
        (Refused | Raises | Tuple, _) | (_, Refused | Raises | Tuple) => Value::of(&[Raises]),
        (Vector, Vector) => Value::of(&[Vector]),
        (Class::Quantity(quantity), Class::Quantity(_) | Int | Float | Boolean) => {
            nonzero_divisor(rhs, Class::Quantity(quantity))
        }
        (Int | Float | Boolean, Class::Quantity(_))
        | (Class::Quantity(_) | Int | Float | Boolean, Text) => Value::of(&[Raises]),
        (Int | Boolean, Int | Boolean) => nonzero_divisor(rhs, Int),
        (Int | Float | Boolean, Int | Float | Boolean) => nonzero_divisor(rhs, Float),
        (Placement | Object, _) | (_, Placement | Object) => Value::any(),
        _ => Value::of(&[Raises]),
    }
}

/// `^` with the right operand's value `exponent` where the file stores it: a
/// quantity object's power by a plain number takes `Unit::pow`, by a
/// quantity object the power truncated to an integer
/// (`Quantity::pow`), and a plain number's power by a quantity object raises
/// (`QuantityPy::number_power_handler`).
fn power(left: Class, right: Class, base: Option<f64>, exponent: Option<f64>) -> Value {
    match (left, right) {
        (Refused | Raises | Tuple, _) | (_, Refused | Raises | Tuple) => Value::of(&[Raises]),
        (Placement | Object, _) | (_, Placement | Object) => Value::any(),
        (Text, _) | (_, Text) | (Int | Float | Boolean, Class::Quantity(_)) => Value::of(&[Raises]),
        (Int | Boolean, Int | Boolean) => match exponent {
            Some(exponent) if exponent < 0.0 => nonzero_divisor(base, Float),
            Some(_) => Value::of(&[Int]),
            None if right == Boolean => Value::of(&[Int]),
            None if base.is_some_and(|base| base != 0.0) => Value::of(&[Int, Float]),
            None => Value::of(&[Int, Float, Raises]),
        },
        (Int | Float | Boolean, Int | Float | Boolean) => match (base, exponent) {
            (Some(0.0), Some(exponent)) if exponent < 0.0 => Value::of(&[Raises]),
            (Some(base), Some(exponent)) if base < 0.0 && exponent.fract() != 0.0 => Value::any(),
            (Some(base), Some(exponent)) if base.powf(exponent).is_finite() => Value::of(&[Float]),
            (Some(base), Some(exponent)) if base.is_finite() && exponent.is_finite() => {
                Value::of(&[Raises])
            }
            (_, Some(0.0)) => Value::of(&[Float]),
            _ => Value::any(),
        },
        (Class::Quantity(base), Int | Float | Boolean) => {
            unit_power(base, exponent, right == Boolean)
        }
        (Class::Quantity(base), Class::Quantity(unit)) => {
            let mut value = match unit {
                Some(Dimensions::REAL) | None => unit_power(
                    base,
                    exponent.filter(|n| n.abs() < 128.0).map(f64::trunc),
                    false,
                ),
                Some(_) => Value::default(),
            };
            if unit != Some(Dimensions::REAL) {
                value.insert(Raises);
            }
            value
        }
        _ => Value::of(&[Raises]),
    }
}

/// A quantity object of unit `base` to the power `exponent` (`Unit::pow`):
/// each exponent of the unit times the power is an integer, or it raises.
/// An exponent the file does not store gives a unit the model does not know,
/// and may overflow unless it is a Boolean.
fn unit_power(base: Option<Dimensions>, exponent: Option<f64>, boolean: bool) -> Value {
    match (base, exponent) {
        (Some(Dimensions::REAL), _) => Value::of(&[Class::Quantity(Some(Dimensions::REAL))]),
        (Some(base), Some(exponent)) => Value::of(&[
            pow_unit(base, exponent).map_or(Raises, |quantity| Class::Quantity(Some(quantity)))
        ]),
        (None, Some(0.0)) => Value::of(&[Class::Quantity(Some(Dimensions::REAL))]),
        (None, Some(1.0)) => Value::of(&[Class::Quantity(None)]),
        (Some(_) | None, None) if boolean => Value::of(&[Class::Quantity(None)]),
        (Some(_) | None, _) => Value::of(&[Class::Quantity(None), Raises]),
    }
}

/// The product of `left` and `right` to the power `sign`, 1 or -1
/// (`Unit::operator*`, `Unit::operator/`).
fn combine(left: Dimensions, right: Dimensions, sign: i8) -> Option<Dimensions> {
    let one = |left: i8, right: i8| exponent(i32::from(left) + i32::from(right) * i32::from(sign));
    left.zip(right, one)
}

/// An exponent of a unit that `Unit::checkRange` takes.
fn exponent(value: i32) -> Option<i8> {
    i8::try_from(value)
        .ok()
        .filter(|value| (-UNIT_EXPONENT_LIMIT..UNIT_EXPONENT_LIMIT).contains(value))
}

/// `quantity` to the power `power` (`Unit::pow`): each product of an
/// exponent of the unit and the power is within `UNIT_POWER` of an integer,
/// and the unit takes the product truncated.
fn pow_unit(quantity: Dimensions, power: f64) -> Option<Dimensions> {
    let one = |value: i8| {
        let product = f64::from(value) * power;
        if (product.round() - product).abs() >= UNIT_POWER {
            return None;
        }
        let truncated = product.trunc();
        (f64::from(i8::MIN)..=f64::from(i8::MAX))
            .contains(&truncated)
            .then(|| (i8::MIN..=i8::MAX).find(|integer| f64::from(*integer) == truncated))
            .flatten()
            .and_then(|integer| exponent(i32::from(integer)))
    };
    quantity.map(one)
}

/// The `n`th root of `quantity` (`Unit::root`): each exponent is divisible
/// by `n`.
fn root(quantity: Dimensions, n: i8) -> Option<Dimensions> {
    let one = |value: i8| (value % n == 0).then_some(value / n);
    quantity.map(one)
}

/// A function that takes each of its first three arguments as a number
/// (`pyToQuantity`) and checks its unit (`FunctionExpression::evaluate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Numeric {
    /// `cos`, `sin`, `tan`: dimensionless or an angle.
    Trigonometric,
    /// `acos`, `asin`, `atan`: dimensionless, an angle.
    Inverse,
    /// `exp`, `log`, `log10`, `sinh`, `tanh`, `cosh`: dimensionless.
    Dimensionless,
    /// `ceil`, `floor`, `abs`: the argument's unit.
    Same,
    Round,
    /// `sqrt` and `cbrt`.
    Root(i8),
    Atan2,
    Mod,
    Pow,
    /// `hypot` and `cath`: arguments of one unit.
    Hypot,
    Not,
    /// `rotationx`, `rotationy`, `rotationz`: a rotation object.
    Rotation,
    /// `translationm` of three lengths: a matrix object.
    Translation,
}

impl Numeric {
    const fn takes(self, count: usize) -> bool {
        match self {
            Self::Atan2 | Self::Mod | Self::Pow => count == 2,
            Self::Hypot => count == 2 || count == 3,
            Self::Translation => count == 1 || count == 3,
            Self::Trigonometric
            | Self::Inverse
            | Self::Dimensionless
            | Self::Same
            | Self::Round
            | Self::Root(_)
            | Self::Not
            | Self::Rotation => count == 1,
        }
    }
}

/// A function that collects the numbers of its arguments
/// (`FunctionExpression::evalAggregate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Collector {
    Sum,
    Average,
    StdDev,
    Min,
    Max,
    Count,
    And,
    Or,
}

impl Collector {
    const fn ignores_units(self) -> bool {
        matches!(self, Self::Count | Self::And | Self::Or)
    }
}

/// What a function does with its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) enum Function {
    Numeric(Numeric),
    /// `list`: a `list` of its arguments.
    List,
    /// `tuple`: a `tuple`, which `calc` refuses.
    Tuple,
    /// `href` and `hiddenref`: the argument.
    Reference,
    /// `str`: the argument's text.
    Text,
    /// `parsequant`: the quantity its text states, which may raise.
    Parse,
    /// `vector`, `matrix`, `placement`, `rotation`: an object made of the
    /// arguments.
    Make(Class),
    /// `create`: an object of the type the first argument names.
    Create,
    /// `minvert`, `mrotate`, `mrotatex`, `mrotatey`, `mrotatez`, `mscale`,
    /// `mtranslate`: a transform of a matrix, placement, or rotation.
    Transform,
    /// A function of vectors (`extractVectorArgument`): how many of its
    /// arguments are vectors, how many after them are lengths, and its
    /// result.
    Vectors {
        vectors: usize,
        lengths: usize,
        result: Class,
    },
}

/// The collector of an aggregate function (`FunctionExpression::evalAggregate`).
fn collector(name: &str) -> Option<Collector> {
    match name {
        "sum" => Some(Collector::Sum),
        "average" => Some(Collector::Average),
        "stddev" => Some(Collector::StdDev),
        "min" => Some(Collector::Min),
        "max" => Some(Collector::Max),
        "count" => Some(Collector::Count),
        "and" => Some(Collector::And),
        "or" => Some(Collector::Or),
        _ => None,
    }
}

fn function(name: &str) -> Option<Function> {
    use Numeric as N;
    const fn vectors(vectors: usize, lengths: usize, result: Class) -> Function {
        Function::Vectors {
            vectors,
            lengths,
            result,
        }
    }
    Some(match name {
        "cos" | "sin" | "tan" => Function::Numeric(N::Trigonometric),
        "acos" | "asin" | "atan" => Function::Numeric(N::Inverse),
        "exp" | "log" | "log10" | "sinh" | "tanh" | "cosh" => Function::Numeric(N::Dimensionless),
        "ceil" | "floor" | "abs" => Function::Numeric(N::Same),
        "round" | "trunc" => Function::Numeric(N::Round),
        "sqrt" => Function::Numeric(N::Root(2)),
        "cbrt" => Function::Numeric(N::Root(3)),
        "atan2" => Function::Numeric(N::Atan2),
        "mod" => Function::Numeric(N::Mod),
        "pow" => Function::Numeric(N::Pow),
        "hypot" | "cath" => Function::Numeric(N::Hypot),
        "not" => Function::Numeric(N::Not),
        "rotationx" | "rotationy" | "rotationz" => Function::Numeric(N::Rotation),
        "translationm" => Function::Numeric(N::Translation),
        "list" => Function::List,
        "tuple" => Function::Tuple,
        "href" | "hiddenref" => Function::Reference,
        "str" => Function::Text,
        "parsequant" => Function::Parse,
        "vector" => Function::Make(Vector),
        "placement" => Function::Make(Placement),
        "matrix" | "rotation" => Function::Make(Object),
        "create" => Function::Create,
        "minvert" | "mrotate" | "mrotatex" | "mrotatey" | "mrotatez" | "mscale" | "mtranslate" => {
            Function::Transform
        }
        "vangle" => vectors(2, 0, Class::Quantity(Some(Dimensions::ANGLE))),
        "vdot" => vectors(2, 0, Float),
        "vcross" => vectors(2, 0, Vector),
        "vnormalize" => vectors(1, 0, Vector),
        "vscale" => vectors(1, 3, Vector),
        "vscalex" | "vscaley" | "vscalez" => vectors(1, 1, Vector),
        "vlinedist" | "vplanedist" => vectors(3, 0, Class::Quantity(Some(Dimensions::LENGTH))),
        "vlinesegdist" | "vlineproj" | "vplaneproj" => vectors(3, 0, Vector),
        _ => return None,
    })
}

fn valid_call(name: &str, function: Function, count: usize) -> bool {
    if count == 0 {
        return false;
    }
    match function {
        Function::Numeric(numeric) => numeric.takes(count),
        Function::Reference | Function::Text | Function::Parse => count == 1,
        Function::Make(Vector) => count == 3,
        Function::Make(Placement) => count <= 3,
        Function::Make(_) if name == "matrix" => count <= 16,
        Function::Make(_) => matches!(count, 2 | 3),
        Function::Transform => match name {
            "minvert" => count == 1,
            "mrotate" => matches!(count, 2..=4),
            "mscale" | "mtranslate" => matches!(count, 2 | 4),
            _ => count == 2,
        },
        Function::Vectors {
            vectors, lengths, ..
        } => count == vectors + lengths,
        Function::List | Function::Tuple | Function::Create => true,
    }
}

/// The value of an argument outside an aggregate: a range is a `list`
/// (`RangeExpression::_getPyValue`).
fn argument(arg: &Argument<'_>, scope: &mut dyn Scope) -> Value {
    match arg {
        Argument::Value(node) => node.value(scope),
        Argument::Range(range) => match scope.range(range.from, range.to) {
            Some(_) => Value::of(&[List]),
            None => Value::of(&[List, Raises]),
        },
    }
}

/// The value of a call (`FunctionExpression::evaluate`).
fn call(function: Function, args: &[Argument<'_>], scope: &mut dyn Scope) -> Value {
    let values = args
        .iter()
        .map(|arg| argument(arg, scope))
        .collect::<Vec<_>>();
    if values.iter().any(|value| value.0 == [Raises]) {
        return Value::of(&[Raises]);
    }
    let raises = values.iter().any(|value| value.contains(Raises));
    let mut value = match function {
        Function::Numeric(Numeric::Translation) if values.len() == 1 => {
            outcomes(Object, sequence_of_three(&values[0]))
        }
        Function::Numeric(numeric) => numeric_call(numeric, &values),
        Function::Reference => return values.first().cloned().unwrap_or(Value::of(&[Raises])),
        Function::List | Function::Tuple => {
            let class = if function == Function::List {
                List
            } else {
                Tuple
            };
            if matches!(args, [Argument::Range(..)]) {
                Value::of(&[class])
            } else {
                Value::items(class, values)
            }
        }
        Function::Text => string_conversion(&values[0]),
        Function::Parse => parse_quantity(&values[0]),
        // FreeCAD returns the allocated object regardless of PyInit status.
        Function::Make(class) => Value::of(&[class]),
        Function::Create => match values[0].text_fact() {
            Some(name) if name.eq_ignore_ascii_case("vector") => Value::of(&[Vector]),
            Some(name) if name.eq_ignore_ascii_case("placement") => Value::of(&[Placement]),
            Some(name)
                if name.eq_ignore_ascii_case("matrix") || name.eq_ignore_ascii_case("rotation") =>
            {
                Value::of(&[Object])
            }
            Some(_) => Value::of(&[Raises]),
            None if values[0].contains(Text) => Value::any(),
            None => Value::of(&[Raises]),
        },
        Function::Transform => Value::any(),
        Function::Vectors {
            vectors,
            lengths,
            result,
        } => vector_call(&values, vectors, lengths, result),
    };
    if raises {
        value.insert(Raises);
    }
    value
}

fn string_conversion(value: &Value) -> Value {
    match value.classes() {
        [Text] => return value.clone(),
        [Int] => {
            if let Some(number) = value.numeric_fact() {
                return Value::text(format!("{number:.0}"));
            }
            if let Some(Fact::WideInteger(text)) = &value.1 {
                return Value::text(text.clone());
            }
        }
        [Boolean] => {
            if let Some(truth) = value.truth() {
                return Value::text(if truth { "True" } else { "False" });
            }
        }
        [Class::Quantity(_)] => {
            // QuantityPy::representation uses a binary64 repr and at most
            // eight fixed unit symbols with one-digit absolute exponents.
            return Value(vec![Text], Some(Fact::TextBound(80)));
        }
        [Refused] if matches!(value.1, Some(Fact::None)) => return Value::text("None"),
        _ => {}
    }
    outcomes(
        Text,
        all_options(value, |class| match class {
            Text | Int | Float | Boolean | Class::Quantity(_) | Vector | Placement => Some(true),
            Refused if matches!(value.1, Some(Fact::None)) => Some(true),
            Refused | List | Tuple | Dict | Object => None,
            Raises => Some(false),
        }),
    )
}

fn outcomes(class: Class, succeeds: Option<bool>) -> Value {
    match succeeds {
        Some(true) => Value::of(&[class]),
        Some(false) => Value::of(&[Raises]),
        None => Value::of(&[class, Raises]),
    }
}

/// The admission check is fixed only where every possible class agrees.
fn all_options(value: &Value, accepts: impl Fn(Class) -> Option<bool>) -> Option<bool> {
    let mut options = value.0.iter().copied().map(accepts);
    let first = options.next()??;
    options.all(|option| option == Some(first)).then_some(first)
}

fn require_all(results: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut all = Some(true);
    for result in results {
        match result {
            Some(false) => return Some(false),
            None => all = None,
            Some(true) => {}
        }
    }
    all
}

fn as_double(value: &Value) -> Option<bool> {
    all_options(value, |class| match class {
        Class::Quantity(_) | Boolean | Float => Some(true),
        Int if value.numeric_fact().is_some() => Some(true),
        Refused if matches!(value.1, Some(Fact::None)) => Some(false),
        Int | Object | Refused => None,
        Text | Vector | Placement | List | Tuple | Dict | Raises => Some(false),
    })
}

fn sequence_of_three(value: &Value) -> Option<bool> {
    all_options(value, |class| match class {
        Vector => Some(true),
        List | Tuple => {
            let items = value.item_facts()?;
            if items.len() == 3 {
                require_all(items.iter().map(as_double))
            } else {
                Some(false)
            }
        }
        Object => None,
        _ => Some(false),
    })
}

fn parse_quantity(value: &Value) -> Value {
    if let Some(text) = value.text_fact() {
        if let Some((number, dimensions)) = super::quantity_literal(text) {
            return Value::numeric(Class::Quantity(Some(dimensions)), Some(number));
        }
    }
    Value::of(&[Class::Quantity(None), Raises])
}

fn numeric_call(function: Numeric, values: &[Value]) -> Value {
    let exponent = values.get(1).and_then(Value::numeric_fact);
    let checked = &values[..values.len().min(3)];
    let mut value = Value::default();
    if checked.iter().any(|value| value.0.is_empty()) {
        return value;
    }
    let mut indices = vec![0_usize; checked.len()];
    loop {
        let classes = indices
            .iter()
            .zip(checked)
            .map(|(index, value)| value.0[*index])
            .collect::<Vec<_>>();
        if classes.iter().all(|class| is_number(*class)) {
            let result = unit_rule(function, &classes, exponent);
            if function == Numeric::Round {
                match values[0].numeric_fact() {
                    Some(number) if number.is_finite() => value.join(result),
                    Some(_) => value.insert(Raises),
                    None => {
                        value.join(result);
                        value.insert(Raises);
                    }
                }
            } else {
                value.join(result);
            }
        } else {
            value.insert(Raises);
        }
        let mut position = 0;
        loop {
            let Some(index) = indices.get_mut(position) else {
                return value;
            };
            *index += 1;
            if *index < checked[position].0.len() {
                break;
            }
            *index = 0;
            position += 1;
        }
    }
}

/// The result of a numeric function of the numbers `classes`, by its unit
/// rule (`FunctionExpression::evaluate`), where the second argument's value
/// is `exponent` where the file stores it.
fn unit_rule(function: Numeric, classes: &[Class], exponent: Option<f64>) -> Value {
    let units = classes.iter().map(|class| unit(*class)).collect::<Vec<_>>();
    let first = units[0];
    let real = Class::Quantity(Some(Dimensions::REAL));
    match function {
        Numeric::Trigonometric => accepts(first, &[Dimensions::REAL, Dimensions::ANGLE], real),
        Numeric::Inverse => accepts(
            first,
            &[Dimensions::REAL],
            Class::Quantity(Some(Dimensions::ANGLE)),
        ),
        Numeric::Dimensionless => accepts(first, &[Dimensions::REAL], real),
        Numeric::Same | Numeric::Round => Value::of(&[Class::Quantity(first)]),
        Numeric::Root(n) => match first {
            Some(quantity) => Value::of(&[
                root(quantity, n).map_or(Raises, |quantity| Class::Quantity(Some(quantity)))
            ]),
            None => Value::of(&[Class::Quantity(None), Raises]),
        },
        Numeric::Atan2 => equal_units(&units, Class::Quantity(Some(Dimensions::ANGLE))),
        Numeric::Hypot => equal_units(&units, Class::Quantity(units.iter().find_map(|unit| *unit))),
        Numeric::Mod => match (first, units[1]) {
            (Some(Dimensions::REAL), _) | (_, Some(Dimensions::REAL)) => {
                Value::of(&[Class::Quantity(first)])
            }
            (Some(left), Some(right)) if left == right => Value::of(&[Class::Quantity(first)]),
            (Some(_), Some(_)) => Value::of(&[Raises]),
            (Some(_) | None, None) | (None, Some(_)) => {
                Value::of(&[Class::Quantity(first), Raises])
            }
        },
        Numeric::Pow => pow_call(first, units[1], classes[1], exponent),
        Numeric::Not => Value::of(&[real]),
        Numeric::Rotation => accepts(first, &[Dimensions::REAL, Dimensions::ANGLE], Object),
        Numeric::Translation => {
            let allowed = [Dimensions::REAL, Dimensions::LENGTH];
            if units
                .iter()
                .any(|unit| unit.is_some_and(|unit| !allowed.contains(&unit)))
            {
                Value::of(&[Raises])
            } else if units.iter().all(Option::is_some) {
                Value::of(&[Object])
            } else {
                Value::of(&[Object, Raises])
            }
        }
    }
}

/// `result` where `unit` is one of `allowed`, and a raise otherwise.
fn accepts(unit: Option<Dimensions>, allowed: &[Dimensions], result: Class) -> Value {
    match unit {
        Some(unit) if allowed.contains(&unit) => Value::of(&[result]),
        Some(_) => Value::of(&[Raises]),
        None => Value::of(&[result, Raises]),
    }
}

/// `result` where each of `units` is one unit, and a raise otherwise.
fn equal_units(units: &[Option<Dimensions>], result: Class) -> Value {
    let known = units.iter().flatten().collect::<Vec<_>>();
    if known.windows(2).any(|pair| pair[0] != pair[1]) {
        Value::of(&[Raises])
    } else if known.len() == units.len() {
        Value::of(&[result])
    } else {
        Value::of(&[result, Raises])
    }
}

/// `pow(base; exponent)`: the exponent is dimensionless, and a base with a
/// unit needs an exponent at most `INTEGRAL` above its rounded value, then
/// takes `Unit::pow`.
fn pow_call(
    base: Option<Dimensions>,
    unit: Option<Dimensions>,
    class: Class,
    exponent: Option<f64>,
) -> Value {
    let mut value = match unit {
        Some(Dimensions::REAL) | None => match (base, exponent) {
            (Some(Dimensions::REAL), _) => Value::of(&[Class::Quantity(Some(Dimensions::REAL))]),
            (base, Some(exponent)) if exponent - exponent.round() >= INTEGRAL => {
                if base.is_none() {
                    Value::of(&[Class::Quantity(Some(Dimensions::REAL)), Raises])
                } else {
                    Value::of(&[Raises])
                }
            }
            (base, exponent) => unit_power(base, exponent, class == Boolean),
        },
        Some(_) => Value::default(),
    };
    if unit != Some(Dimensions::REAL) {
        value.insert(Raises);
    }
    value
}

/// The value of a function of vectors: each of the first `vectors`
/// arguments is a `VectorPy` (`extractVectorArgument`), and each of the
/// `lengths` after them is a length or dimensionless
/// (`extractLengthValueArgument`).
fn vector_call(values: &[Value], vectors: usize, lengths: usize, result: Class) -> Value {
    if values.len() < vectors + lengths {
        return Value::of(&[Raises]);
    }
    let mut value = Value::default();
    let mut succeeds = true;
    for (index, argument) in values.iter().take(vectors + lengths).enumerate() {
        let mut accepted = false;
        for class in &argument.0 {
            let (accepts, raises) = if index < vectors {
                (*class == Vector, *class != Vector)
            } else if is_number(*class) {
                match unit(*class) {
                    Some(Dimensions::REAL | Dimensions::LENGTH) => (true, false),
                    Some(_) => (false, true),
                    None => (true, true),
                }
            } else {
                (false, true)
            };
            accepted |= accepts;
            if raises {
                value.insert(Raises);
            }
        }
        succeeds &= accepted;
    }
    if succeeds {
        value.insert(result);
    }
    value
}

/// The unit a collector takes from its first value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum First {
    Empty,
    Of(Option<Dimensions>),
}

/// The state of a collector after some values, with its count capped at two
/// for the standard-deviation arity check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Collected {
    unit: First,
    count: u8,
}

/// The value of an aggregate (`FunctionExpression::evalAggregate`): a value
/// argument that is a number is collected and any other is passed over, a
/// cell of a range is collected where it holds a number and raises
/// otherwise, and the collector raises on values of different units.
fn aggregate(collector: Collector, args: &[Argument<'_>], scope: &mut dyn Scope) -> Value {
    let mut states = vec![Collected {
        unit: First::Empty,
        count: 0,
    }];
    let mut value = Value::default();
    let mut folded = Some(Folded::default());
    for arg in args {
        match arg {
            Argument::Value(node) => {
                let argument = node.value(scope);
                folded = folded.and_then(|fold| fold.collect(collector, &argument));
                let mut next = Vec::new();
                for class in argument.0 {
                    match class {
                        Raises => value.insert(Raises),
                        Text | Vector | Placement | List | Dict | Tuple | Object | Refused => {
                            for state in &states {
                                push(&mut next, *state);
                            }
                        }
                        Class::Quantity(_) | Int | Float | Boolean => {
                            for state in &states {
                                collect(collector, *state, class, &mut next, &mut value);
                            }
                        }
                    }
                }
                states = next;
            }
            Argument::Range(range) => {
                let Some(cells) = scope.range(range.from, range.to) else {
                    value.join(if collector.ignores_units() {
                        Value::of(&[Int, Raises])
                    } else {
                        Value::of(&[Class::Quantity(None), Int, Float, Raises])
                    });
                    return value;
                };
                for cell in cells {
                    folded = folded.and_then(|fold| fold.collect(collector, &cell));
                    let mut next = Vec::new();
                    for class in cell.0 {
                        match class {
                            Class::Quantity(_) | Int | Float => {
                                for state in &states {
                                    collect(collector, *state, class, &mut next, &mut value);
                                }
                            }
                            Boolean | Text | Vector | Placement | List | Dict | Tuple | Object
                            | Refused | Raises => {
                                value.insert(Raises);
                            }
                        }
                    }
                    states = next;
                }
            }
        }
    }
    let folded = folded.and_then(|fold| fold.finish(collector));
    for state in states {
        value.join(collected(collector, state, folded));
    }
    value
}

fn push(states: &mut Vec<Collected>, state: Collected) {
    if !states.contains(&state) {
        states.push(state);
    }
}

/// The states after `state` collects a number of class `class`.
fn collect(
    collector: Collector,
    state: Collected,
    class: Class,
    next: &mut Vec<Collected>,
    value: &mut Value,
) {
    let unit = unit(class);
    if collector == Collector::StdDev && state.unit == First::Empty {
        match unit {
            Some(unit) if combine(unit, unit, 1).is_none() => {
                value.insert(Raises);
                return;
            }
            None => value.insert(Raises),
            Some(_) => {}
        }
    }
    let counted = Collected {
        count: (state.count + 1).min(2),
        ..state
    };
    if collector.ignores_units() {
        push(next, counted);
        return;
    }
    match state.unit {
        First::Empty => push(
            next,
            Collected {
                unit: First::Of(unit),
                ..counted
            },
        ),
        First::Of(Some(first)) if unit == Some(first) => push(next, counted),
        First::Of(Some(_)) if unit.is_some() => value.insert(Raises),
        First::Of(first) => {
            value.insert(Raises);
            push(
                next,
                Collected {
                    unit: First::Of(first.or(unit)),
                    ..counted
                },
            );
        }
    }
}

/// The result of a collector in `state` (`pyFromQuantity`), where `folded`
/// is the value of an aggregate of constants.
fn collected(collector: Collector, state: Collected, folded: Option<f64>) -> Value {
    if collector.ignores_units() {
        return Value::numeric(Int, folded);
    }
    match (collector, state.unit) {
        (Collector::StdDev, _) if state.count < 2 => Value::of(&[Raises]),
        (Collector::Average, First::Empty) => Value::numeric(Float, Some(f64::NAN)),
        (_, First::Empty) => Value::numeric(Int, Some(0.0)),
        (_, First::Of(Some(Dimensions::REAL))) => match folded {
            Some(folded) => Value::numeric(Value::number(folded), Some(folded)),
            None => Value::of(&[Int, Float]),
        },
        (_, First::Of(Some(quantity))) => Value::numeric(Class::Quantity(Some(quantity)), folded),
        (_, First::Of(None)) => Value::of(&[Class::Quantity(None), Int, Float]),
    }
}

#[derive(Default)]
struct Folded {
    count: u32,
    number: f64,
}

impl Folded {
    fn collect(mut self, collector: Collector, value: &Value) -> Option<Self> {
        if value.contains(Raises) {
            return None;
        }
        if value.0.iter().all(|class| !is_number(*class)) {
            return Some(self);
        }
        if !value.0.iter().all(|class| is_number(*class)) {
            return None;
        }
        if collector != Collector::Count {
            let number = value.numeric_fact()?;
            self.number = match collector {
                Collector::Sum | Collector::Average => self.number + number,
                Collector::Min if self.count == 0 || number < self.number => number,
                Collector::Max if self.count == 0 || number > self.number => number,
                Collector::Min | Collector::Max => self.number,
                Collector::Count | Collector::And | Collector::Or | Collector::StdDev => {
                    return None
                }
            };
        }
        self.count = self.count.checked_add(1)?;
        Some(self)
    }

    fn finish(&self, collector: Collector) -> Option<f64> {
        match collector {
            Collector::Count => Some(f64::from(self.count)),
            Collector::Average => Some(self.number / f64::from(self.count)),
            Collector::Sum | Collector::Min | Collector::Max => Some(self.number),
            Collector::StdDev | Collector::And | Collector::Or => None,
        }
    }
}

#[derive(Debug, Clone)]
enum Token<'a> {
    Number {
        text: &'a str,
        number: f64,
        integer: Option<i64>,
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
    Ignored,
}

#[derive(Debug, Clone)]
struct Lexeme<'a> {
    token: Token<'a>,
    start: usize,
    end: usize,
}

/// The tokens of `text` as `FreeCAD`'s lexer reads them: the longest match,
/// and on a tie the earlier rule.
fn lex(text: &str) -> (Vec<Lexeme<'_>>, Result<(), Unparsed>) {
    let text = text.split('\0').next().unwrap_or(text);
    let mut lexemes = Vec::new();
    let mut at = 0;
    while let Some(ch) = text[at..].chars().next() {
        if matches!(ch, ' ' | '\t' | '\n') {
            at += 1;
            continue;
        }
        let (token, length) = match lexeme(&text[at..]) {
            Ok(token) => token,
            Err(error) => return (lexemes, Err(error)),
        };
        let start = at;
        at += length;
        if matches!(token, Token::Ignored) {
            continue;
        }
        if lexemes.len() == MAX_TOKENS {
            return (lexemes, Err(Unparsed::Limit));
        }
        lexemes.push(Lexeme {
            token,
            start,
            end: at,
        });
    }
    (lexemes, Ok(()))
}

fn lexeme(rest: &str) -> Result<(Token<'_>, usize), Unparsed> {
    let ch = rest.chars().next().ok_or(Unparsed::Syntax)?;
    if rest.starts_with("<<") {
        if let Some(length) = string(rest) {
            return Ok((Token::Text(&rest[..length]), length));
        }
    }
    for (symbol, operator) in [
        ("==", Operator::Equal),
        ("!=", Operator::NotEqual),
        (">=", Operator::GreaterEqual),
        ("<=", Operator::LessEqual),
        ("<", Operator::Less),
        (">", Operator::Greater),
    ] {
        if rest.starts_with(symbol) {
            return Ok((Token::Compare(operator), symbol.len()));
        }
    }
    let lexeme = match ch {
        '-' | '\u{2212}' => (Token::Minus, ch.len_utf8()),
        '\'' | '"' => (
            Token::UsUnit(unit_word(&rest[..1]).ok_or(Unparsed::Syntax)?),
            1,
        ),
        '°' | '′' | '″' => (
            Token::Unit(unit_word(&rest[..ch.len_utf8()]).ok_or(Unparsed::Syntax)?),
            ch.len_utf8(),
        ),
        '$' => cell_address(rest).map_or((Token::Ignored, 1), |length| {
            (Token::Cell(&rest[..length]), length)
        }),
        '.' | ',' | '0'..='9' => number(rest)?.unwrap_or((Token::Symbol(ch), 1)),
        '%' | '+' | '(' | ')' | '=' | '/' | '*' | '^' | '{' | '}' | '[' | ']' | ':' | ';' | '@'
        | '?' | '#' => (Token::Symbol(ch), 1),
        ch if identifier::is_letter(ch) || ch == '_' => word(rest),
        // Flex's default rule consumes unmatched bytes (Expression.l).
        _ => (Token::Ignored, ch.len_utf8()),
    };
    Ok(lexeme)
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

/// Expression.l:332–342, separate from the C complete-number pass.
fn number(rest: &str) -> Result<Option<(Token<'_>, usize)>, Unparsed> {
    let bytes = rest.as_bytes();
    let digits = |at: usize| {
        bytes[at..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let whole = digits(0);
    let mut end = whole;
    if matches!(bytes.get(end), Some(b'.' | b',')) {
        let fraction = digits(end + 1);
        if fraction != 0 {
            end += 1 + fraction;
        }
    }
    if end == 0 {
        return Ok(None);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let start = end + 1 + usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(start);
        if exponent != 0 {
            end = start + exponent;
        }
    }
    let text = &rest[..end];
    let (number, integer) = if whole == end {
        let integer = text
            .parse::<i64>()
            .ok()
            .filter(|&value| value < i64::MAX)
            .ok_or(Unparsed::Syntax)?;
        let number = text.parse::<f64>().map_err(|_| Unparsed::Syntax)?;
        (number, Some(integer))
    } else {
        // Expression.cpp:3559–3585 returns zero when the local buffer fills.
        let number = if text.len() > 39 {
            0.0
        } else {
            let number = text
                .replace(',', ".")
                .parse::<f64>()
                .map_err(|_| Unparsed::Syntax)?;
            let nonzero = text
                .split(['e', 'E'])
                .next()
                .is_some_and(|mantissa| mantissa.bytes().any(|byte| matches!(byte, b'1'..=b'9')));
            if !number.is_finite() || number == 0.0 && nonzero {
                return Err(Unparsed::Syntax);
            }
            number
        };
        (number, (number == 1.0).then_some(1))
    };
    Ok(Some((
        Token::Number {
            text,
            number,
            integer,
        },
        end,
    )))
}

pub(super) fn number_prefix(text: &str) -> Option<(f64, usize)> {
    let (Token::Number { number, .. }, length) = number(text).ok()?? else {
        return None;
    };
    Some((number, length))
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
    let identifier = identifier::prefix(rest, false);
    let name = identifier::prefix(rest, true);
    let after = rest[name..].trim_start_matches(['\t', '\u{10}']);
    let function = (name != 0 && after.starts_with('('))
        .then_some((Token::Function(&rest[..name]), rest.len() - after.len() + 1));
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
        function,
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

    /// Backtrack on a mismatched production. A resource limit is terminal.
    fn attempt<T>(
        &mut self,
        rule: impl FnOnce(&mut Self) -> Result<T, Unparsed>,
    ) -> Result<Option<T>, Unparsed> {
        let at = self.at;
        match rule(self) {
            Ok(found) => Ok(Some(found)),
            Err(Unparsed::Syntax) => {
                self.at = at;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// `rule` one level deeper.
    fn nested<T>(
        &mut self,
        rule: impl FnOnce(&mut Self) -> Result<T, Unparsed>,
    ) -> Result<T, Unparsed> {
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
                Operator::Divide => match self.attempt(Self::unit_suffix)? {
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
            Token::Number { .. } | Token::Constant { .. } => self.number(),
            Token::Text(text) if !matches!(self.peek_at(1), Some(Token::Symbol('.' | '#'))) => {
                self.at += 1;
                Ok(Node::Text(text))
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

    fn number(&mut self) -> Result<Node<'a>, Unparsed> {
        let start = self.at;
        self.at += 1;
        let suffix = self.attempt(Self::unit_suffix)?;
        let bare = suffix.as_ref().is_some_and(|(_, bare)| *bare);
        let first = Self::number_node(&self.lexemes[start].token, suffix.map(|(unit, _)| unit))?;
        if let (
            true,
            Some(token @ (Token::Number { .. } | Token::Constant { .. })),
            Some(Token::UsUnit(unit)),
        ) = (bare, self.peek_at(0), self.peek_at(1))
        {
            let second = Self::number_node(token, Some(unit.clone()))?;
            self.at += 2;
            return Ok(Node::Binary {
                op: Operator::Add,
                left: Box::new(first),
                right: Box::new(second),
            });
        }
        Ok(first)
    }

    fn number_node(token: &Token<'a>, unit: Option<Unit>) -> Result<Node<'a>, Unparsed> {
        match token {
            Token::Number { text, number, .. } => Ok(Node::Number {
                text,
                number: *number,
                unit,
            }),
            Token::Constant { name, value, .. } => Ok(Node::Constant {
                name,
                value: *value,
                unit,
            }),
            _ => Err(Unparsed::Syntax),
        }
    }

    /// A unit atom with optional integer powers, or a group of units.
    fn unit_suffix(&mut self) -> Result<(Unit, bool), Unparsed> {
        let (mut unit, mut bare) = match self.peek() {
            Some(Token::Unit(unit)) => {
                let unit = unit.clone();
                self.at += 1;
                (unit, false)
            }
            Some(Token::UsUnit(unit)) => {
                let unit = unit.clone();
                self.at += 1;
                (unit, true)
            }
            Some(Token::Symbol('(')) => {
                self.at += 1;
                let unit = self.nested(Self::unit_expression)?;
                self.require(')')?;
                (unit, false)
            }
            _ => return Err(Unparsed::Syntax),
        };
        while self.eat('^') {
            let negative = matches!(self.peek(), Some(Token::Minus));
            self.at += usize::from(negative);
            let Some(Token::Number {
                integer: Some(power),
                ..
            }) = self.peek()
            else {
                return Err(Unparsed::Syntax);
            };
            let power = if negative { -*power } else { *power };
            self.at += 1;
            unit = unit.power(power);
            bare = false;
        }
        Ok((unit, bare))
    }

    /// Unit products and quotients within a unit expression.
    fn unit_expression(&mut self) -> Result<Unit, Unparsed> {
        let mut unit = self.unit_suffix()?.0;
        loop {
            let sign = if self.eat('*') {
                1
            } else if self.eat('/') {
                -1
            } else {
                return Ok(unit);
            };
            unit = unit.times(self.unit_suffix()?.0, sign);
        }
    }

    fn span(&self, at: usize) -> std::ops::Range<usize> {
        self.lexemes
            .get(at)
            .map_or(0..0, |lexeme| lexeme.start..lexeme.end)
    }

    /// Recover completed paths from a bounded token prefix. Use the same path
    /// grammar as a complete expression, with no guessed binding for non-path tokens.
    fn partial_paths(&mut self) -> Vec<Path> {
        let mut paths = Vec::new();
        self.at = 0;
        while !self.done() {
            let at = self.at;
            if matches!(
                self.peek(),
                Some(Token::Identifier(_) | Token::Cell(_) | Token::Text(_) | Token::Symbol('.'))
            ) {
                if let Ok(node) = self.path() {
                    for reference in node.references() {
                        if let Reference::Path(path) = reference {
                            paths.push(path.clone());
                        }
                    }
                }
            }
            self.at = self.at.max(at + 1);
        }
        paths
    }

    fn call(&mut self, name: &'a str) -> Result<Node<'a>, Unparsed> {
        let collector = collector(name);
        let function = function(name);
        if collector.is_none() && function.is_none() {
            return Err(Unparsed::Syntax);
        }
        let args = self.arguments()?;
        if let Some(collector) = collector {
            return Ok(Node::Aggregate { collector, args });
        }
        let function = function
            .filter(|&function| valid_call(name, function, args.len()))
            .ok_or(Unparsed::Syntax)?;
        Ok(Node::Call { function, args })
    }

    fn arguments(&mut self) -> Result<Vec<Argument<'a>>, Unparsed> {
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
                let spans = [self.span(self.at), self.span(self.at + 2)];
                self.at += 3;
                args.push(Argument::Range(CellRange { from, to, spans }));
            } else {
                args.push(Argument::Value(self.expression()?));
            }
            if self.eat(',') || self.eat(';') {
                continue;
            }
            self.require(')')?;
            return Ok(args);
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
        if path.components.len() > 1 || path.origin != Origin::Named {
            while self.symbol('.') && matches!(self.peek_at(1), Some(Token::Identifier(_))) {
                self.at += 1;
                path.components.push(self.id_or_cell()?);
            }
        }
        path.span.end = self.span(self.at - 1).end;
        if !self.eat('[') {
            return Ok(Node::Path(path));
        }
        let mut arguments = Vec::new();
        loop {
            self.indexer(&mut arguments)?;
            while self.symbol('.') && matches!(self.peek_at(1), Some(Token::Identifier(_))) {
                self.at += 2;
            }
            if !self.eat('[') {
                break;
            }
        }
        Ok(Node::Indexed(path, arguments))
    }

    fn indexer(&mut self, arguments: &mut Vec<Node<'a>>) -> Result<(), Unparsed> {
        let start = !self.symbol(':') && !self.symbol(']');
        if start {
            arguments.push(self.expression()?);
        }
        if self.eat(':') {
            let stop = !self.symbol(':') && !self.symbol(']');
            if stop {
                arguments.push(self.expression()?);
            }
            if self.eat(':') {
                arguments.push(self.expression()?);
            } else if !start && !stop {
                return Err(Unparsed::Syntax);
            }
        } else if !start {
            return Err(Unparsed::Syntax);
        }
        self.require(']')
    }

    /// The object of a path: a `<<label>>`, or a name.
    fn object(&mut self, path: &mut Path) -> Result<(), Unparsed> {
        let name = match self.peek() {
            Some(Token::Text(quoted)) => {
                if path.origin != Origin::Outside {
                    path.origin = Origin::Label;
                }
                unquote(quoted).to_string()
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

#[cfg(test)]
mod tests;
