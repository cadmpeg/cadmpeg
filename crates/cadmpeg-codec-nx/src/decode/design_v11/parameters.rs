// SPDX-License-Identifier: Apache-2.0
//! NX expressions as the part's user parameters.
//!
//! An NX expression declares its unit in its header and stores its formula
//! text. A formula that is one number is a literal in the declared unit. A
//! formula of references becomes a neutral tree where every construct
//! translates under NX's semantics and the tree has the declared quantity;
//! every other formula keeps its text, with each name it resolves as a
//! reference.

use std::collections::BTreeMap;
use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::edit::Value;
use cadmpeg_ir::design::expr::{
    self, BinaryOp, Expression, ExpressionDialect, ExpressionToken, Function, MulOp, Neutral, Node,
    UnaryOp,
};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::projection::fields::{ModelParameterField, ParameterField};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::DesignParameter as LegacyParameter;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::native::NativeConvertError;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};

/// The parameters of the scope of `definition`, one per decoded expression,
/// each placed by the record order of the one expression table that holds
/// them.
pub(super) fn project(
    ir: &mut CadIr,
    definition: &ProductDefinitionId,
) -> Result<(), NativeConvertError> {
    let places = match ir.native.namespace("nx") {
        Some(namespace) => crate::native::om::expression_places(namespace)?,
        None => BTreeMap::new(),
    };
    let mut tables = BTreeMap::<Option<&FeatureId>, BTreeMap<&str, Vec<&LegacyParameter>>>::new();
    for parameter in &ir.model.parameters {
        tables
            .entry(parameter.owner.as_ref())
            .or_default()
            .entry(parameter.name.as_str())
            .or_default()
            .push(parameter);
    }
    let mut parameters = Vec::with_capacity(ir.model.parameters.len());
    for parameter in &ir.model.parameters {
        let Some(name) = NonBlankString::new(parameter.name.clone()) else {
            continue;
        };
        let declared = declared(parameter);
        let names = &tables[&parameter.owner.as_ref()];
        let definition_expression = expression(&parameter.expression, &declared, names);
        let value = literal_value(&parameter.expression, &declared);
        parameters.push(DesignParameter {
            id: parameter.id.clone(),
            scope: definition.clone(),
            owner: None,
            position: parameter
                .native_ref
                .as_ref()
                .and_then(|expression| places.get(expression))
                .copied(),
            name: Some(name),
            kind: declared.kind,
            unit: declared.unit,
            definition: ParameterDefinition::Expression(definition_expression),
            value,
            display: None,
            tolerance: None,
            decimals: None,
            comment: None,
            native_options: BTreeMap::default(),
            source_settings: BTreeMap::default(),
            native_ref: parameter.native_ref.clone(),
        });
    }
    ir.design.parameters.extend(parameters);
    ir.design.parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// The model parameter fields [`project`] reads to build `field`. The
/// declared unit is in `properties`; the names a formula resolves are those
/// of the parameters with its owner.
#[deny(clippy::wildcard_enum_match_arm)]
pub(super) fn inputs(field: ParameterField) -> &'static [ModelParameterField] {
    match field {
        ParameterField::Name => &[ModelParameterField::Name],
        ParameterField::Kind | ParameterField::Unit => &[ModelParameterField::Properties],
        ParameterField::Definition => &[
            ModelParameterField::ExpressionText,
            ModelParameterField::Properties,
            ModelParameterField::Owner,
        ],
        ParameterField::Value => &[
            ModelParameterField::ExpressionText,
            ModelParameterField::Properties,
        ],
        ParameterField::NativeRef | ParameterField::Position => &[ModelParameterField::NativeRef],
        ParameterField::Scope
        | ParameterField::Owner
        | ParameterField::Display
        | ParameterField::Tolerance
        | ParameterField::Decimals
        | ParameterField::Comment
        | ParameterField::NativeOptions
        | ParameterField::SourceSettings => &[],
    }
}

/// A parameter's declared quantity and unit, from its expression header.
struct Declared {
    kind: ValueKind,
    unit: Option<Unit>,
}

/// The declared quantity of a unit token whose symbols are unit symbols, the
/// newton, or the hertz; every other token is a native kind.
fn declared(parameter: &LegacyParameter) -> Declared {
    let token = match parameter.properties.get("unit").map_or("", String::as_str) {
        "millimeter" => "mm",
        "inch" => "in",
        "degree" => "degrees",
        token => token,
    };
    let quantity = declared_unit(token).and_then(|unit| {
        let (_, quantity) = canonical(1.0, &unit)?;
        Some((quantity, unit))
    });
    match quantity {
        Some((quantity, unit)) => Declared {
            kind: ValueKind::Quantity(quantity),
            unit: Some(unit),
        },
        None => Declared {
            kind: ValueKind::Native(cadmpeg_core::nonblank_literal!("nx:{token}")),
            unit: None,
        },
    }
}

/// An NX unit token as a product of unit symbols: factors joined by `*`, a
/// denominator after each `/`, a power after `^`, and an optional display
/// label in parentheses.
fn declared_unit(token: &str) -> Option<Unit> {
    let token = match token.split_once('(') {
        Some((head, label)) if label.ends_with(')') => head,
        _ => token,
    };
    let mut factors = Vec::<UnitFactor>::new();
    for (index, part) in token.split('/').enumerate() {
        let sign: i8 = if index == 0 { 1 } else { -1 };
        if index == 0 && part == "1" {
            continue;
        }
        for factor in part.split('*') {
            let (name, power) = match factor.split_once('^') {
                Some((name, power)) => (name, power.parse::<i8>().ok()?),
                None => (factor, 1),
            };
            let expansion: Vec<(UnitSymbol, i8)> = match name {
                "N" => vec![(UnitSymbol::Kg, 1), (UnitSymbol::M, 1), (UnitSymbol::S, -2)],
                "Hz" => vec![(UnitSymbol::S, -1)],
                "degrees" => vec![(UnitSymbol::Deg, 1)],
                name => vec![(
                    *UnitSymbol::ALL
                        .iter()
                        .find(|symbol| symbol.name() == name)?,
                    1,
                )],
            };
            for (symbol, base) in expansion {
                let power = base.checked_mul(power)?.checked_mul(sign)?;
                match factors.iter_mut().find(|known| known.symbol == symbol) {
                    Some(known) => {
                        known.power = NonZeroI8::new(known.power.get().checked_add(power)?)?;
                    }
                    None => factors.push(UnitFactor {
                        symbol,
                        power: NonZeroI8::new(power)?,
                    }),
                }
            }
        }
    }
    (!factors.is_empty()).then_some(Unit(factors))
}

/// `value` in `unit` as the canonical value of its quantity.
fn canonical(value: f64, unit: &Unit) -> Option<(f64, Quantity)> {
    let literal = Node::Number {
        value: NonNegativeReal::new(value)?,
        unit: unit.clone(),
    };
    match cadmpeg_ir::design::edit::evaluate(&literal, &mut |_| None).ok()? {
        Value::Number { value, quantity } => Some((value, quantity)),
        Value::Boolean(_) | Value::Text(_) => None,
    }
}

fn unit_of(symbol: UnitSymbol) -> Option<Unit> {
    NonZeroI8::new(1).map(|power| Unit(vec![UnitFactor { symbol, power }]))
}

/// A formula that is one number: whether it is negated, and its magnitude.
fn literal(text: &str) -> Option<(bool, f64)> {
    let text = text.trim();
    let (negated, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits.trim_start()),
        None => (false, text),
    };
    if !digits.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return None;
    }
    Some((negated, digits.parse::<f64>().ok()?))
}

/// The stored value of a formula that is one number in the declared unit:
/// the literal is its own value. The decoder states no value it evaluates
/// from other expressions.
fn literal_value(text: &str, declared: &Declared) -> Option<ParameterValue> {
    let (negated, magnitude) = literal(text)?;
    let (value, _) = canonical(magnitude, declared.unit.as_ref()?)?;
    FiniteReal::new(if negated { -value } else { value }).map(ParameterValue::Number)
}

/// A formula that is one number, in the unit the expression declares (§5.13:
/// a literal that is the whole expression of a parameter whose unit the
/// source declares is in that unit by the source's own statement).
fn literal_tree(text: &str, declared: &Declared) -> Option<Node<ParameterRef>> {
    let (negated, magnitude) = literal(text)?;
    let number = Node::Number {
        value: NonNegativeReal::new(magnitude)?,
        unit: declared.unit.clone()?,
    };
    Some(if negated {
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(number),
        }
    } else {
        number
    })
}

/// One lexical token of NX formula text.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Token<'a> {
    Number(&'a str),
    Name(&'a str),
    Mark(char),
    Space(&'a str),
}

fn tokens(text: &str) -> Option<Vec<(usize, Token<'_>)>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        let token = if byte.is_ascii_whitespace() {
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            Token::Space(&text[start..at])
        } else if byte.is_ascii_digit() || byte == b'.' {
            while bytes
                .get(at)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
            {
                at += 1;
            }
            if matches!(bytes.get(at), Some(b'e' | b'E'))
                && bytes
                    .get(at + 1)
                    .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-'))
            {
                at += 2;
                while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    at += 1;
                }
            }
            Token::Number(&text[start..at])
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            while bytes
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                at += 1;
            }
            Token::Name(&text[start..at])
        } else if matches!(byte, b'+' | b'-' | b'*' | b'/' | b'^' | b'(' | b')' | b',') {
            at += 1;
            Token::Mark(char::from(byte))
        } else {
            return None;
        };
        out.push((start, token));
    }
    Some(out)
}

/// The expression of a formula: the neutral tree where it translates, else
/// the source text with each resolved name as a reference.
fn expression(
    text: &str,
    declared: &Declared,
    names: &BTreeMap<&str, Vec<&LegacyParameter>>,
) -> Expression {
    let resolve = |name: &str| match names.get(name).map(Vec::as_slice) {
        Some([target]) => Some(*target),
        _ => None,
    };
    if let Some(neutral) =
        literal_tree(text, declared).or_else(|| neutral(text, &declared.kind, &resolve))
    {
        return Expression::Neutral(Neutral(neutral));
    }
    let mut tokens_out = Vec::new();
    let mut pending = String::new();
    let lexed = tokens(text);
    let mut cursor = 0;
    for (start, token) in lexed.iter().flatten() {
        if let Token::Name(name) = token {
            if let Some(target) = resolve(name) {
                pending.push_str(&text[cursor..*start]);
                if !pending.is_empty() {
                    tokens_out.push(ExpressionToken::Text(std::mem::take(&mut pending)));
                }
                tokens_out.push(ExpressionToken::Reference(reference(&target.id)));
                cursor = start + name.len();
            }
        }
    }
    pending.push_str(&text[cursor..]);
    if !pending.is_empty() {
        tokens_out.push(ExpressionToken::Text(pending));
    }
    Expression::Source {
        dialect: ExpressionDialect::Nx,
        tokens: tokens_out,
    }
}

/// What a closing parenthesis ends in the canonical text.
#[derive(Clone, Copy)]
enum Group {
    Plain,
    /// NX `deg(x)` returns the number `x · 180 / π`.
    Degrees,
}

/// The neutral tree of a formula with no number that translates exactly and
/// has the declared quantity. A formula that holds a number among other
/// terms is read under Rule H1, and NX stores no value to test it against
/// (§5.13), so it keeps its source text.
fn neutral<'a>(
    text: &str,
    kind: &ValueKind,
    resolve: &dyn Fn(&str) -> Option<&'a LegacyParameter>,
) -> Option<Node<ParameterRef>> {
    let ValueKind::Quantity(expected) = kind else {
        return None;
    };
    let lexed = tokens(text)?;
    let mut canonical = String::new();
    let mut refs = Vec::<ParameterRef>::new();
    let mut quantities = Vec::new();
    let mut groups = Vec::new();
    let mut tokens = lexed
        .iter()
        .map(|(_, token)| *token)
        .filter(|token| !matches!(token, Token::Space(_)))
        .peekable();
    while let Some(token) = tokens.next() {
        match token {
            Token::Space(_) => {}
            Token::Number(_) => return None,
            Token::Name(name) if tokens.peek() == Some(&Token::Mark('(')) => {
                tokens.next();
                match name {
                    "pi" => {
                        (tokens.next() == Some(Token::Mark(')'))).then_some(())?;
                        canonical.push_str("pi");
                        continue;
                    }
                    "deg" => {
                        canonical.push_str("((");
                        groups.push(Group::Degrees);
                        continue;
                    }
                    "sin" | "cos" | "sqrt" | "abs" | "min" | "max" => canonical.push_str(name),
                    _ => return None,
                }
                canonical.push('(');
                groups.push(Group::Plain);
            }
            Token::Name(name) => {
                let target = resolve(name)?;
                let reference = reference(&target.id);
                let index = match refs.iter().position(|known| *known == reference) {
                    Some(index) => index,
                    None => {
                        refs.push(reference);
                        quantities.push(quantity_of(&declared(target).kind));
                        refs.len() - 1
                    }
                };
                canonical.push('$');
                canonical.push_str(&index.to_string());
            }
            Token::Mark('(') => {
                canonical.push('(');
                groups.push(Group::Plain);
            }
            Token::Mark(')') => match groups.pop()? {
                Group::Plain => canonical.push(')'),
                Group::Degrees => canonical.push_str(") * 1[rad] / 1[deg])"),
            },
            Token::Mark(mark) => {
                canonical.push(' ');
                canonical.push(mark);
                canonical.push(' ');
            }
        }
    }
    groups.is_empty().then_some(())?;
    let mut tree = expr::parse(&canonical, &refs, &[]).ok()?;
    let lookup = |reference: &ParameterRef| {
        refs.iter()
            .position(|known| known == reference)
            .and_then(|index| quantities.get(index).copied().flatten())
    };
    (typed(&mut tree, &lookup)? == *expected).then_some(tree)
}

fn quantity_of(kind: &ValueKind) -> Option<Quantity> {
    match kind {
        ValueKind::Quantity(quantity) => Some(*quantity),
        _ => None,
    }
}

/// The quantity of a translated tree under §5.13's operator rules. NX reads
/// the argument of `sin` and `cos` in degrees, so a dimensionless argument
/// becomes an angle in degrees.
fn typed(
    tree: &mut Node<ParameterRef>,
    lookup: &dyn Fn(&ParameterRef) -> Option<Quantity>,
) -> Option<Quantity> {
    match tree {
        Node::Number { unit, .. } => unit_quantity(unit),
        Node::Constant(_) => Some(Quantity::REAL),
        Node::Reference(reference) => lookup(reference),
        Node::Unary {
            op: UnaryOp::Negate,
            operand,
        } => typed(operand, lookup),
        Node::Binary { op, left, right } => {
            let a = typed(left, lookup)?;
            let b = typed(right, lookup)?;
            match op {
                BinaryOp::Power if a == Quantity::REAL && b == Quantity::REAL => {
                    Some(Quantity::REAL)
                }
                _ => None,
            }
        }
        Node::Sum { first, rest } => {
            let a = typed(first, lookup)?;
            for (_, operand) in rest.iter_mut() {
                (typed(operand, lookup)? == a).then_some(())?;
            }
            Some(a)
        }
        Node::Product { first, rest } => {
            let mut held = typed(first, lookup)?;
            for (op, operand) in rest.iter_mut() {
                let b = typed(operand, lookup)?;
                held = match op {
                    MulOp::Multiply => combine(held, b, i8::checked_add),
                    MulOp::Divide => combine(held, b, i8::checked_sub),
                }?;
            }
            Some(held)
        }
        Node::Call { function, args } => match (function, args.as_mut_slice()) {
            (Function::Sin | Function::Cos, [argument]) => {
                let quantity = typed(argument, lookup)?;
                if quantity == Quantity::REAL {
                    let operand = std::mem::replace(argument, Node::Constant(expr::Constant::Pi));
                    *argument = Node::mul(
                        operand,
                        MulOp::Multiply,
                        Node::Number {
                            value: NonNegativeReal::new(1.0)?,
                            unit: unit_of(UnitSymbol::Deg)?,
                        },
                    );
                } else if quantity != Quantity::ANGLE {
                    return None;
                }
                Some(Quantity::REAL)
            }
            (Function::Sqrt, [argument]) => scale(typed(argument, lookup)?, 0.5),
            (Function::Abs, [argument]) => typed(argument, lookup),
            (Function::Min | Function::Max, [first, rest @ ..]) if !rest.is_empty() => {
                let quantity = typed(first, lookup)?;
                for argument in rest {
                    (typed(argument, lookup)? == quantity).then_some(())?;
                }
                Some(quantity)
            }
            _ => None,
        },
        _ => None,
    }
}

fn unit_quantity(unit: &Unit) -> Option<Quantity> {
    canonical(1.0, unit).map(|(_, quantity)| quantity)
}

fn combine(a: Quantity, b: Quantity, op: fn(i8, i8) -> Option<i8>) -> Option<Quantity> {
    Some(Quantity {
        length: op(a.length, b.length)?,
        mass: op(a.mass, b.mass)?,
        angle: op(a.angle, b.angle)?,
        time: op(a.time, b.time)?,
    })
}

/// A quantity raised to `exponent`, where every scaled exponent is an
/// integer.
fn scale(quantity: Quantity, exponent: f64) -> Option<Quantity> {
    let power = |value: i8| {
        let scaled = f64::from(value) * exponent;
        if scaled.fract() != 0.0 {
            return None;
        }
        i8::try_from(cadmpeg_core::convert::truncate_f64_to_i32(scaled)?).ok()
    };
    Some(Quantity {
        length: power(quantity.length)?,
        mass: power(quantity.mass)?,
        angle: power(quantity.angle)?,
        time: power(quantity.time)?,
    })
}

fn reference(id: &ParameterId) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(id.clone()),
        through: None,
    }
}

#[cfg(test)]
mod tests;
