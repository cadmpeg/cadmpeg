// SPDX-License-Identifier: Apache-2.0
//! Parsing canonical expression text.

use std::num::NonZeroI8;

use super::super::units::{Unit, UnitFactor, UnitSymbol};
use super::{
    AddOp, BendVariable, BinaryOp, Constant, ExprError, Function, Measure, MulOp, Node, UnaryOp,
};
use crate::bounds::MAX_EXPRESSION_DEPTH;
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

    /// The operator that continues a chain of `spine` at the next token.
    fn chain_op(&self, spine: Spine) -> Option<Link> {
        match spine {
            Spine::Or => self.at_word("or").then_some(Link::Or),
            Spine::And => self.at_word("and").then_some(Link::And),
            Spine::Sum if self.at_mark("+") => Some(Link::Add(AddOp::Add)),
            Spine::Sum if self.at_mark("-") => Some(Link::Add(AddOp::Subtract)),
            Spine::Prod if self.at_mark("*") => Some(Link::Mul(MulOp::Multiply)),
            Spine::Prod if self.at_mark("/") => Some(Link::Mul(MulOp::Divide)),
            Spine::Sum | Spine::Prod => None,
        }
    }

    fn comparison_op(&self) -> Option<BinaryOp> {
        match self.peek() {
            Some(Tok::Mark("==")) => Some(BinaryOp::Equal),
            Some(Tok::Mark("!=")) => Some(BinaryOp::NotEqual),
            Some(Tok::Mark("<")) => Some(BinaryOp::Less),
            Some(Tok::Mark("<=")) => Some(BinaryOp::LessEqual),
            Some(Tok::Mark(">")) => Some(BinaryOp::Greater),
            Some(Tok::Mark(">=")) => Some(BinaryOp::GreaterEqual),
            _ => None,
        }
    }

    /// Reads an `expr`. Each rule partly read waits as a frame on a work
    /// stack, so the depth of the tree does not reach the thread's stack.
    fn expr(&mut self) -> Result<Node<R>, ExprError> {
        let mut frames = Vec::new();
        let mut rule = Rule::Expr;
        loop {
            let mut node = self.descend(rule, &mut frames)?;
            loop {
                let Some(frame) = frames.pop() else {
                    return Ok(node);
                };
                match self.resume(frame, node, &mut frames)? {
                    Next::Node(read) => node = read,
                    Next::Read(next) => {
                        rule = next;
                        break;
                    }
                }
            }
        }
    }

    /// Enters `rule` and the rules it starts with, down to the atom they
    /// start with, and reads that atom, or enters the `expr` it opens.
    fn descend(
        &mut self,
        mut rule: Rule,
        frames: &mut Vec<Frame<Node<R>>>,
    ) -> Result<Node<R>, ExprError> {
        loop {
            rule = match rule {
                Rule::Expr => {
                    frames.push(Frame::chain(Spine::Or));
                    Rule::And
                }
                Rule::And => {
                    frames.push(Frame::chain(Spine::And));
                    Rule::Not
                }
                Rule::Not => {
                    if self.at_word("not") {
                        self.i += 1;
                        frames.push(Frame::Not);
                        Rule::Not
                    } else {
                        frames.push(Frame::Cmp(None));
                        Rule::Sum
                    }
                }
                Rule::Sum => {
                    frames.push(Frame::chain(Spine::Sum));
                    Rule::Prod
                }
                Rule::Prod => {
                    frames.push(Frame::chain(Spine::Prod));
                    Rule::Neg
                }
                Rule::Neg => {
                    if self.at_mark("-") {
                        self.i += 1;
                        frames.push(Frame::Neg);
                        Rule::Neg
                    } else {
                        frames.push(Frame::Pow(None));
                        match self.atom(frames)? {
                            Some(node) => return Ok(node),
                            None => Rule::Expr,
                        }
                    }
                }
            };
        }
    }

    /// Gives `frame` the node it waits for: the rule ends with a node, or
    /// waits again, on the stack, for the rule it reads next.
    fn resume(
        &mut self,
        frame: Frame<Node<R>>,
        node: Node<R>,
        frames: &mut Vec<Frame<Node<R>>>,
    ) -> Result<Next<Node<R>>, ExprError> {
        Ok(match frame {
            Frame::Chain { spine, left } => {
                let node = match left {
                    None => node,
                    Some((left, link)) => link.join(left, node),
                };
                let Some(op) = self.chain_op(spine) else {
                    return Ok(Next::Node(node));
                };
                self.i += 1;
                frames.push(Frame::Chain {
                    spine,
                    left: Some((node, op)),
                });
                Next::Read(spine.operand())
            }
            Frame::Not => Next::Node(Node::Unary {
                op: UnaryOp::Not,
                operand: Box::new(node),
            }),
            Frame::Cmp(None) => match self.comparison_op() {
                Some(op) => {
                    self.i += 1;
                    frames.push(Frame::Cmp(Some((node, op))));
                    Next::Read(Rule::Sum)
                }
                None => Next::Node(node),
            },
            Frame::Cmp(Some((left, op))) => Next::Node(Node::binary(op, left, node)),
            Frame::Neg => Next::Node(Node::Unary {
                op: UnaryOp::Negate,
                operand: Box::new(node),
            }),
            Frame::Pow(None) => {
                if self.at_mark("^") {
                    self.i += 1;
                    frames.push(Frame::Pow(Some(node)));
                    Next::Read(Rule::Neg)
                } else {
                    Next::Node(node)
                }
            }
            Frame::Pow(Some(left)) => Next::Node(Node::binary(BinaryOp::Power, left, node)),
            Frame::Paren => {
                self.expect_mark(")")?;
                Next::Node(node)
            }
            Frame::If(mut parts) => {
                parts.push(node);
                if parts.len() < 3 {
                    self.expect_mark(",")?;
                    frames.push(Frame::If(parts));
                    Next::Read(Rule::Expr)
                } else {
                    self.expect_mark(")")?;
                    let [condition, then, otherwise] = <[Node<R>; 3]>::try_from(parts)
                        .map_err(|_| ExprError::Malformed("an if states three parts".into()))?;
                    Next::Node(Node::branch(condition, then, otherwise))
                }
            }
            Frame::Call(function, mut args) => {
                args.push(node);
                if self.at_mark(",") {
                    self.i += 1;
                    frames.push(Frame::Call(function, args));
                    Next::Read(Rule::Expr)
                } else {
                    self.expect_mark(")")?;
                    Next::Node(Node::Call { function, args })
                }
            }
        })
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

    /// Reads an atom, or opens one that holds an `expr` and returns `None`:
    /// a parenthesis, an `if`, or a call, which waits as a frame.
    fn atom(&mut self, frames: &mut Vec<Frame<Node<R>>>) -> Result<Option<Node<R>>, ExprError> {
        let node = match self.next() {
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
                Node::Number { value, unit }
            }
            Some(Tok::Str(value)) => Node::Text(value),
            Some(Tok::Ref(index)) => match self.refs.get(index) {
                Some(reference) => Node::Reference(reference.clone()),
                None => return malformed(format!("${index} names no entry of refs")),
            },
            Some(Tok::Meas(index)) => match self.measures.get(index) {
                Some(measure) => Node::Measure(Box::new(measure.clone())),
                None => return malformed(format!("@{index} names no entry of measures")),
            },
            Some(Tok::Mark("(")) => {
                frames.push(Frame::Paren);
                return Ok(None);
            }
            Some(Tok::Word(word)) => match word.as_str() {
                "pi" => Node::Constant(Constant::Pi),
                "e" => Node::Constant(Constant::E),
                "true" => Node::Boolean(true),
                "false" => Node::Boolean(false),
                "t" => Node::LawParameter,
                "bendangle" => Node::Bend(BendVariable::Angle),
                "bendradius" => Node::Bend(BendVariable::InsideRadius),
                "bendthickness" => Node::Bend(BendVariable::Thickness),
                "if" => {
                    self.expect_mark("(")?;
                    frames.push(Frame::If(Vec::with_capacity(3)));
                    return Ok(None);
                }
                name => {
                    let function = Function::ALL
                        .into_iter()
                        .find(|function| function.name() == name)
                        .ok_or_else(|| {
                            ExprError::Malformed(format!("word {name:?} is no keyword or function"))
                        })?;
                    self.expect_mark("(")?;
                    frames.push(Frame::Call(function, Vec::new()));
                    return Ok(None);
                }
            },
            other => return malformed(format!("unexpected {other:?}")),
        };
        Ok(Some(node))
    }
}

/// A rule of the grammar the parser enters next.
#[derive(Clone, Copy)]
enum Rule {
    Expr,
    And,
    Not,
    Sum,
    Prod,
    Neg,
}

/// A left-associative chain of operators of one precedence.
#[derive(Clone, Copy)]
enum Spine {
    Or,
    And,
    Sum,
    Prod,
}

impl Spine {
    /// The rule each operand of the chain reads.
    const fn operand(self) -> Rule {
        match self {
            Self::Or => Rule::And,
            Self::And => Rule::Not,
            Self::Sum => Rule::Prod,
            Self::Prod => Rule::Neg,
        }
    }
}

/// The operator that joins the next operand to a chain.
#[derive(Clone, Copy)]
enum Link {
    Add(AddOp),
    Mul(MulOp),
    And,
    Or,
}

impl Link {
    /// `left` and `right` as one chain node, `right` joining the run of
    /// `left` where `left` is a chain of the same level.
    fn join<R>(self, left: Node<R>, right: Node<R>) -> Node<R> {
        match self {
            Self::Add(op) => Node::add(left, op, right),
            Self::Mul(op) => Node::mul(left, op, right),
            Self::And => Node::and(left, right),
            Self::Or => Node::or(left, right),
        }
    }
}

/// A rule partly read, waiting for the node it reads next.
enum Frame<N> {
    /// A chain, with the operands read so far and the operator that follows
    /// them.
    Chain {
        spine: Spine,
        left: Option<(N, Link)>,
    },
    /// `not` before its operand.
    Not,
    /// A comparison, with its left operand and operator once read.
    Cmp(Option<(N, BinaryOp)>),
    /// `-` before its operand.
    Neg,
    /// A power, with its base once read.
    Pow(Option<N>),
    /// `(` before its `expr`.
    Paren,
    /// An `if`, with the parts read so far.
    If(Vec<N>),
    /// A call, with the arguments read so far.
    Call(Function, Vec<N>),
}

impl<N> Frame<N> {
    const fn chain(spine: Spine) -> Self {
        Self::Chain { spine, left: None }
    }
}

/// What a frame does with the node it waited for.
enum Next<N> {
    /// The frame's rule ends with this node.
    Node(N),
    /// The frame waits again for a node of this rule.
    Read(Rule),
}

/// Parses canonical text against its side lists. `$i` is `refs[i]` and `@i`
/// is `measures[i]`.
///
/// # Errors
///
/// Returns [`ExprError::Malformed`] for text outside the grammar of
/// canonical text, and for text whose tree is deeper than the 64 levels of
/// an expression (§2.5).
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
    let depth = node.depth();
    if depth > MAX_EXPRESSION_DEPTH {
        crate::bounds::note_expression_refused();
        return malformed(format!(
            "the expression is {depth} levels deep, past the {MAX_EXPRESSION_DEPTH} levels of an \
             expression"
        ));
    }
    Ok(node)
}
