// SPDX-License-Identifier: Apache-2.0
//! Parsing canonical expression text.

use std::num::NonZeroI8;

use super::super::units::{Unit, UnitFactor, UnitSymbol};
use super::{level, BendVariable, BinaryOp, Constant, ExprError, Function, Measure, Node, UnaryOp};
use crate::scalar::NonNegativeReal;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(String),
    Word(String),
    Str(String),
    Ref(usize),
    Meas(usize),
    Mark(&'static str),
}

const MARKS: [&str; 16] = [
    "==", "!=", "<=", ">=", "<", ">", "(", ")", "[", "]", ",", "+", "-", "*", "/", "^",
];

fn malformed<T>(message: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError::Malformed(message.into()))
}

fn digits_end(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    at
}

fn tokens(text: &str) -> Result<Vec<Tok>, ExprError> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = bytes.get(i) {
        if c == b' ' {
            i += 1;
        } else if c.is_ascii_digit() {
            let mut j = digits_end(bytes, i);
            if bytes.get(j) == Some(&b'.') && bytes.get(j + 1).is_some_and(u8::is_ascii_digit) {
                j = digits_end(bytes, j + 1);
            }
            if matches!(bytes.get(j), Some(b'e' | b'E')) {
                let mut k = j + 1;
                if matches!(bytes.get(k), Some(b'+' | b'-')) {
                    k += 1;
                }
                if bytes.get(k).is_some_and(u8::is_ascii_digit) {
                    j = digits_end(bytes, k);
                }
            }
            out.push(Tok::Num(text[i..j].to_owned()));
            i = j;
        } else if c.is_ascii_alphabetic() {
            let mut j = i + 1;
            while bytes.get(j).is_some_and(u8::is_ascii_alphanumeric) {
                j += 1;
            }
            out.push(Tok::Word(text[i..j].to_owned()));
            i = j;
        } else if c == b'"' {
            let mut j = i + 1;
            let mut value = String::new();
            loop {
                match text[j..].chars().next() {
                    None => return malformed(format!("unterminated string at {i}")),
                    Some('"') => {
                        j += 1;
                        break;
                    }
                    Some('\\') => match text[j + 1..].chars().next() {
                        Some(escaped @ ('"' | '\\')) => {
                            value.push(escaped);
                            j += 2;
                        }
                        _ => return malformed(format!("bad escape at {j}")),
                    },
                    Some(other) => {
                        value.push(other);
                        j += other.len_utf8();
                    }
                }
            }
            out.push(Tok::Str(value));
            i = j;
        } else if c == b'$' || c == b'@' {
            let j = digits_end(bytes, i + 1);
            if j == i + 1 {
                return malformed(format!("{} without index at {i}", char::from(c)));
            }
            let index = text[i + 1..j]
                .parse::<usize>()
                .map_err(|error| ExprError::Malformed(error.to_string()))?;
            out.push(if c == b'$' {
                Tok::Ref(index)
            } else {
                Tok::Meas(index)
            });
            i = j;
        } else if let Some(mark) = MARKS.iter().find(|mark| text[i..].starts_with(**mark)) {
            out.push(Tok::Mark(mark));
            i += mark.len();
        } else {
            return malformed(format!("unexpected {:?} at {i}", text[i..].chars().next()));
        }
    }
    Ok(out)
}

struct Parser<'a, R> {
    toks: Vec<Tok>,
    i: usize,
    refs: &'a [R],
    measures: &'a [Measure],
}

impl<R: Clone> Parser<'_, R> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn at_mark(&self, mark: &str) -> bool {
        matches!(self.peek(), Some(Tok::Mark(m)) if *m == mark)
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w == word)
    }

    fn next(&mut self) -> Option<Tok> {
        let tok = self.toks.get(self.i).cloned();
        self.i += 1;
        tok
    }

    fn expect_mark(&mut self, mark: &str) -> Result<(), ExprError> {
        if self.at_mark(mark) {
            self.i += 1;
            Ok(())
        } else {
            malformed(format!("expected {mark}, got {:?}", self.peek()))
        }
    }

    fn binary(op: BinaryOp, left: Node<R>, right: Node<R>) -> Node<R> {
        Node::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn expr(&mut self) -> Result<Node<R>, ExprError> {
        let _level = level()?;
        let mut node = self.and()?;
        while self.at_word("or") {
            self.i += 1;
            node = Self::binary(BinaryOp::Or, node, self.and()?);
        }
        Ok(node)
    }

    fn and(&mut self) -> Result<Node<R>, ExprError> {
        let mut node = self.not()?;
        while self.at_word("and") {
            self.i += 1;
            node = Self::binary(BinaryOp::And, node, self.not()?);
        }
        Ok(node)
    }

    fn not(&mut self) -> Result<Node<R>, ExprError> {
        if self.at_word("not") {
            let _level = level()?;
            self.i += 1;
            return Ok(Node::Unary {
                op: UnaryOp::Not,
                operand: Box::new(self.not()?),
            });
        }
        self.cmp()
    }

    fn cmp(&mut self) -> Result<Node<R>, ExprError> {
        let node = self.sum()?;
        let op = match self.peek() {
            Some(Tok::Mark("==")) => BinaryOp::Equal,
            Some(Tok::Mark("!=")) => BinaryOp::NotEqual,
            Some(Tok::Mark("<")) => BinaryOp::Less,
            Some(Tok::Mark("<=")) => BinaryOp::LessEqual,
            Some(Tok::Mark(">")) => BinaryOp::Greater,
            Some(Tok::Mark(">=")) => BinaryOp::GreaterEqual,
            _ => return Ok(node),
        };
        self.i += 1;
        Ok(Self::binary(op, node, self.sum()?))
    }

    fn sum(&mut self) -> Result<Node<R>, ExprError> {
        let mut node = self.prod()?;
        loop {
            let op = if self.at_mark("+") {
                BinaryOp::Add
            } else if self.at_mark("-") {
                BinaryOp::Subtract
            } else {
                return Ok(node);
            };
            self.i += 1;
            node = Self::binary(op, node, self.prod()?);
        }
    }

    fn prod(&mut self) -> Result<Node<R>, ExprError> {
        let mut node = self.neg()?;
        loop {
            let op = if self.at_mark("*") {
                BinaryOp::Multiply
            } else if self.at_mark("/") {
                BinaryOp::Divide
            } else {
                return Ok(node);
            };
            self.i += 1;
            node = Self::binary(op, node, self.neg()?);
        }
    }

    fn neg(&mut self) -> Result<Node<R>, ExprError> {
        if self.at_mark("-") {
            let _level = level()?;
            self.i += 1;
            return Ok(Node::Unary {
                op: UnaryOp::Negate,
                operand: Box::new(self.neg()?),
            });
        }
        self.pow()
    }

    fn pow(&mut self) -> Result<Node<R>, ExprError> {
        let node = self.atom()?;
        if self.at_mark("^") {
            let _level = level()?;
            self.i += 1;
            return Ok(Self::binary(BinaryOp::Power, node, self.neg()?));
        }
        Ok(node)
    }

    fn unit(&mut self) -> Result<Unit, ExprError> {
        let mut factors = Vec::new();
        let mut sign: i32 = 1;
        loop {
            let symbol = match self.next() {
                Some(Tok::Word(word)) => UnitSymbol::ALL
                    .into_iter()
                    .find(|symbol| symbol.name() == word)
                    .ok_or_else(|| ExprError::Malformed(format!("not a unit symbol: {word}")))?,
                other => return malformed(format!("expected a unit symbol, got {other:?}")),
            };
            let mut power: i32 = 1;
            if self.at_mark("^") {
                self.i += 1;
                let negative = self.at_mark("-");
                if negative {
                    self.i += 1;
                }
                power = match self.next() {
                    Some(Tok::Num(n)) => n.parse::<i32>().map_err(|error| {
                        ExprError::Malformed(format!("unit power {n}: {error}"))
                    })?,
                    other => return malformed(format!("expected a power, got {other:?}")),
                };
                if negative {
                    power = -power;
                }
            }
            let signed = sign * power;
            let power = i8::try_from(signed)
                .ok()
                .and_then(NonZeroI8::new)
                .ok_or_else(|| {
                    ExprError::Malformed(format!("unit power {signed} is not a nonzero i8"))
                })?;
            factors.push(UnitFactor { symbol, power });
            if self.at_mark("*") || self.at_mark("/") {
                sign = if self.at_mark("*") { 1 } else { -1 };
                self.i += 1;
            } else {
                return Ok(Unit(factors));
            }
        }
    }

    fn atom(&mut self) -> Result<Node<R>, ExprError> {
        match self.next() {
            Some(Tok::Num(text)) => {
                let value = text
                    .parse::<f64>()
                    .map_err(|error| ExprError::Malformed(error.to_string()))?;
                let value = NonNegativeReal::new(value)
                    .ok_or_else(|| ExprError::Malformed(format!("{text} is not finite")))?;
                let mut unit = Unit(Vec::new());
                if self.at_mark("[") {
                    self.i += 1;
                    unit = self.unit()?;
                    self.expect_mark("]")?;
                }
                Ok(Node::Number { value, unit })
            }
            Some(Tok::Str(value)) => Ok(Node::Text(value)),
            Some(Tok::Ref(index)) => match self.refs.get(index) {
                Some(reference) => Ok(Node::Reference(reference.clone())),
                None => malformed(format!("${index} names no entry of refs")),
            },
            Some(Tok::Meas(index)) => match self.measures.get(index) {
                Some(measure) => Ok(Node::Measure(Box::new(measure.clone()))),
                None => malformed(format!("@{index} names no entry of measures")),
            },
            Some(Tok::Mark("(")) => {
                let node = self.expr()?;
                self.expect_mark(")")?;
                Ok(node)
            }
            Some(Tok::Word(word)) => match word.as_str() {
                "pi" => Ok(Node::Constant(Constant::Pi)),
                "e" => Ok(Node::Constant(Constant::E)),
                "true" => Ok(Node::Boolean(true)),
                "false" => Ok(Node::Boolean(false)),
                "t" => Ok(Node::LawParameter),
                "bendangle" => Ok(Node::Bend(BendVariable::Angle)),
                "bendradius" => Ok(Node::Bend(BendVariable::InsideRadius)),
                "bendthickness" => Ok(Node::Bend(BendVariable::Thickness)),
                "if" => {
                    self.expect_mark("(")?;
                    let condition = self.expr()?;
                    self.expect_mark(",")?;
                    let then = self.expr()?;
                    self.expect_mark(",")?;
                    let otherwise = self.expr()?;
                    self.expect_mark(")")?;
                    Ok(Node::If(Box::new([condition, then, otherwise])))
                }
                name => {
                    let function = Function::ALL
                        .into_iter()
                        .find(|function| function.name() == name)
                        .ok_or_else(|| {
                            ExprError::Malformed(format!("word {name:?} is no keyword or function"))
                        })?;
                    self.expect_mark("(")?;
                    let mut args = vec![self.expr()?];
                    while self.at_mark(",") {
                        self.i += 1;
                        args.push(self.expr()?);
                    }
                    self.expect_mark(")")?;
                    Ok(Node::Call { function, args })
                }
            },
            other => malformed(format!("unexpected {other:?}")),
        }
    }
}

/// Parses canonical text against its side lists. `$i` is `refs[i]` and `@i`
/// is `measures[i]`.
///
/// # Errors
///
/// Returns [`ExprError::Malformed`] for text outside the grammar of
/// canonical text, and [`ExprError::ResourceLimit`] for text nested past the
/// expression ceiling.
pub fn parse<R: Clone>(text: &str, refs: &[R], measures: &[Measure]) -> Result<Node<R>, ExprError> {
    let mut parser = Parser {
        toks: tokens(text)?,
        i: 0,
        refs,
        measures,
    };
    let node = parser.expr()?;
    if parser.i != parser.toks.len() {
        return malformed(format!("trailing tokens {:?}", parser.toks.get(parser.i..)));
    }
    Ok(node)
}
