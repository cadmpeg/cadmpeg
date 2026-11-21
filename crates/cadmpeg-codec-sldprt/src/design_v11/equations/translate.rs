// SPDX-License-Identifier: Apache-2.0
//! An equation's expression as a neutral tree (§5.13): numbers with or
//! without a unit, quoted names, `+ - * /`, unary minus, and parentheses.
//! Any other construct has no tree here.
//!
//! The `SolidWorks` dialect is unit-blind. A bare number, or a global
//! variable, that is an operand of `+` or `-` beside a quantity, or that is
//! the whole expression of a quantity parameter, has that quantity: a length
//! in the definition's length unit, an angle in degrees. A global variable so
//! used becomes `$i * 1[u]`. Every other bare number is dimensionless.
//! A non-blind operand's quantity follows from its own operands alone.

use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{BinaryOp, Node, UnaryOp};
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};

use super::Names;
use crate::design_v11::parameters::{canonical, number_node, LITERAL_UNITS};

/// The deepest nesting of parentheses and unary minus the parser reads.
const MAX_DEPTH: usize = 64;
/// The most tokens an expression with a tree holds, which bounds the tree's
/// depth.
const MAX_TOKENS: usize = 256;

/// What a tree is typed against.
pub(super) struct Typing<'a> {
    pub(super) parameters: &'a [DesignParameter],
    pub(super) names: &'a Names,
    /// The kind of the parameter the expression defines.
    pub(super) target: &'a ValueKind,
    pub(super) length_unit: Option<UnitSymbol>,
}

/// The neutral tree of `expression`, where every construct translates and the
/// tree's quantity is the target's.
pub(super) fn tree(expression: &str, typing: &Typing<'_>) -> Option<Node<ParameterRef>> {
    let tokens = tokens(expression, typing.names)?;
    if tokens.len() > MAX_TOKENS {
        return None;
    }
    let mut parser = Parser { tokens, at: 0 };
    let ast = parser.sum(0)?;
    if parser.at != parser.tokens.len() {
        return None;
    }
    let quantity = kind_quantity(typing.target)?;
    let (node, found) = typing.node(&ast, Some(quantity), true)?;
    (found == quantity).then_some(node)
}

/// The tree, quantity, and canonical value of an expression that is one
/// number, negated or not, with or without a unit: a bare number is
/// dimensionless.
pub(super) fn literal(expression: &str) -> Option<(Node<ParameterRef>, Quantity, f64)> {
    let mut rest = expression.trim();
    let negative = match rest.strip_prefix('-') {
        Some(after) => {
            rest = after.trim_start();
            true
        }
        None => false,
    };
    let (Token::Number(value, unit), after) = number(rest)? else {
        return None;
    };
    if !after.trim().is_empty() {
        return None;
    }
    let (quantity, scale) = match unit {
        Some(unit) => canonical(unit)?,
        None => (Quantity::REAL, 1.0),
    };
    let signed = if negative { -value } else { value };
    Some((number_node(signed, unit)?, quantity, signed * scale))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Token {
    Number(f64, Option<UnitSymbol>),
    Name(usize),
    Operator(u8),
    Open,
    Close,
}

fn tokens(expression: &str, names: &Names) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut rest = expression;
    while let Some(character) = rest.chars().next() {
        match character {
            ' ' | '\t' => rest = &rest[1..],
            '+' | '-' | '*' | '/' => {
                tokens.push(Token::Operator(rest.as_bytes()[0]));
                rest = &rest[1..];
            }
            '(' => {
                tokens.push(Token::Open);
                rest = &rest[1..];
            }
            ')' => {
                tokens.push(Token::Close);
                rest = &rest[1..];
            }
            '"' => {
                let end = rest[1..].find('"')? + 1;
                if rest[end + 1..].starts_with('"') {
                    return None;
                }
                tokens.push(Token::Name(names.resolve(&rest[1..end])?));
                rest = &rest[end + 1..];
            }
            '0'..='9' | '.' => {
                let (token, after) = number(rest)?;
                tokens.push(token);
                rest = after;
            }
            _ => return None,
        }
    }
    Some(tokens)
}

/// The number that opens `text`, with the unit its suffix names, and the
/// text after it.
fn number(text: &str) -> Option<(Token, &str)> {
    let bytes = text.as_bytes();
    let mut end = bytes
        .iter()
        .position(|byte| !(byte.is_ascii_digit() || *byte == b'.'))
        .unwrap_or(bytes.len());
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let digits = bytes[end + 1 + sign..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits > 0 {
            end += 1 + sign + digits;
        }
    }
    let value = text[..end]
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())?;
    let rest = &text[end..];
    let suffix_end = rest
        .char_indices()
        .find(|(_, character)| !(character.is_alphabetic() || *character == '\u{00b0}'))
        .map_or(rest.len(), |(at, _)| at);
    let suffix = &rest[..suffix_end];
    let unit = if suffix.is_empty() {
        None
    } else {
        Some(
            LITERAL_UNITS
                .iter()
                .find(|(name, _)| *name == suffix)
                .map(|(_, symbol)| *symbol)?,
        )
    };
    Some((Token::Number(value, unit), &rest[suffix_end..]))
}

/// How a unit-blind number reads at its place.
#[derive(Debug, Clone, Copy)]
enum Blind {
    Dimensionless,
    Unit(UnitSymbol),
}

#[derive(Debug, Clone, PartialEq)]
enum Ast {
    Number(f64, Option<UnitSymbol>),
    Name(usize),
    Negate(Box<Ast>),
    Binary(u8, Box<Ast>, Box<Ast>),
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn next_operator(&self, accepted: &[u8]) -> Option<u8> {
        match self.tokens.get(self.at) {
            Some(Token::Operator(operator)) if accepted.contains(operator) => Some(*operator),
            _ => None,
        }
    }

    fn sum(&mut self, depth: usize) -> Option<Ast> {
        let mut left = self.product(depth)?;
        while let Some(operator) = self.next_operator(b"+-") {
            self.at += 1;
            let right = self.product(depth)?;
            left = Ast::Binary(operator, Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn product(&mut self, depth: usize) -> Option<Ast> {
        let mut left = self.unary(depth)?;
        while let Some(operator) = self.next_operator(b"*/") {
            self.at += 1;
            let right = self.unary(depth)?;
            left = Ast::Binary(operator, Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn unary(&mut self, depth: usize) -> Option<Ast> {
        if depth >= MAX_DEPTH {
            return None;
        }
        let token = *self.tokens.get(self.at)?;
        self.at += 1;
        match token {
            Token::Operator(b'-') => Some(Ast::Negate(Box::new(self.unary(depth + 1)?))),
            Token::Number(value, unit) => Some(Ast::Number(value, unit)),
            Token::Name(index) => Some(Ast::Name(index)),
            Token::Open => {
                let inner = self.sum(depth + 1)?;
                (self.tokens.get(self.at) == Some(&Token::Close)).then(|| {
                    self.at += 1;
                    inner
                })
            }
            Token::Operator(_) | Token::Close => None,
        }
    }
}

impl Typing<'_> {
    fn is_global(&self, index: usize) -> bool {
        let parameter = &self.parameters[index];
        parameter.owner.is_none() && parameter.kind == ValueKind::Quantity(Quantity::REAL)
    }

    /// Whether `ast` is unit-blind: a bare number, a global variable, or a
    /// negation or sum of such.
    fn is_blind(&self, ast: &Ast) -> bool {
        match ast {
            Ast::Number(_, unit) => unit.is_none(),
            Ast::Name(index) => self.is_global(*index),
            Ast::Negate(operand) => self.is_blind(operand),
            Ast::Binary(b'+' | b'-', left, right) => self.is_blind(left) && self.is_blind(right),
            Ast::Binary(..) => false,
        }
    }

    /// How a unit-blind number reads as a `quantity`.
    fn blind(&self, quantity: Quantity) -> Option<Blind> {
        match quantity {
            Quantity::REAL => Some(Blind::Dimensionless),
            Quantity::LENGTH => Some(Blind::Unit(self.length_unit?)),
            Quantity::ANGLE => Some(Blind::Unit(UnitSymbol::Deg)),
            _ => None,
        }
    }

    /// The tree of `ast` and its quantity. `want` is the quantity its place
    /// takes, which a unit-blind operand has where `blind` holds.
    fn node(
        &self,
        ast: &Ast,
        want: Option<Quantity>,
        blind: bool,
    ) -> Option<(Node<ParameterRef>, Quantity)> {
        let place = want.filter(|_| blind).unwrap_or(Quantity::REAL);
        match ast {
            Ast::Number(value, Some(unit)) => {
                Some((number_node(*value, Some(*unit))?, canonical(*unit)?.0))
            }
            Ast::Number(value, None) => {
                let unit = match self.blind(place)? {
                    Blind::Dimensionless => None,
                    Blind::Unit(unit) => Some(unit),
                };
                Some((number_node(*value, unit)?, place))
            }
            Ast::Name(index) => {
                let parameter = &self.parameters[*index];
                let reference = Node::Reference(ParameterRef {
                    target: RefTarget::Parameter(parameter.id.clone()),
                    through: None,
                });
                if self.is_global(*index) {
                    return Some(match self.blind(place)? {
                        Blind::Unit(unit) => (
                            Node::Binary {
                                op: BinaryOp::Multiply,
                                left: Box::new(reference),
                                right: Box::new(number_node(1.0, Some(unit))?),
                            },
                            place,
                        ),
                        Blind::Dimensionless => (reference, Quantity::REAL),
                    });
                }
                Some((reference, kind_quantity(&parameter.kind)?))
            }
            Ast::Negate(operand) => {
                let (node, quantity) = self.node(operand, want, blind)?;
                Some((
                    Node::Unary {
                        op: UnaryOp::Negate,
                        operand: Box::new(node),
                    },
                    quantity,
                ))
            }
            Ast::Binary(operator @ (b'+' | b'-'), left, right) => {
                let typed = |operand: &Ast| {
                    if self.is_blind(operand) {
                        Some(None)
                    } else {
                        self.node(operand, None, false).map(Some)
                    }
                };
                let (left_typed, right_typed) = (typed(left)?, typed(right)?);
                let shared = left_typed
                    .as_ref()
                    .or(right_typed.as_ref())
                    .map_or(place, |(_, quantity)| *quantity);
                let (left, first) = match left_typed {
                    Some(typed) => typed,
                    None => self.node(left, Some(shared), true)?,
                };
                let (right, second) = match right_typed {
                    Some(typed) => typed,
                    None => self.node(right, Some(shared), true)?,
                };
                if first != shared || second != shared {
                    return None;
                }
                let op = if *operator == b'+' {
                    BinaryOp::Add
                } else {
                    BinaryOp::Subtract
                };
                Some((
                    Node::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    shared,
                ))
            }
            Ast::Binary(operator, left, right) => {
                let (left, first) = self.node(left, None, false)?;
                let (right, second) = self.node(right, None, false)?;
                let (op, sign) = if *operator == b'*' {
                    (BinaryOp::Multiply, 1)
                } else {
                    (BinaryOp::Divide, -1)
                };
                Some((
                    Node::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    combined(first, second, sign)?,
                ))
            }
        }
    }
}

/// The quantity of a parameter's value in an expression: its quantity, or
/// dimensionless for an integer.
fn kind_quantity(kind: &ValueKind) -> Option<Quantity> {
    match kind {
        ValueKind::Quantity(quantity) => Some(*quantity),
        ValueKind::Integer => Some(Quantity::REAL),
        ValueKind::Boolean | ValueKind::Text | ValueKind::Native(_) => None,
    }
}

/// `left` times `right` raised to `sign`.
fn combined(left: Quantity, right: Quantity, sign: i8) -> Option<Quantity> {
    let term = |a: i8, b: i8| a.checked_add(b.checked_mul(sign)?);
    Some(Quantity {
        length: term(left.length, right.length)?,
        mass: term(left.mass, right.mass)?,
        angle: term(left.angle, right.angle)?,
        time: term(left.time, right.time)?,
    })
}
