// SPDX-License-Identifier: Apache-2.0
//! Parameter expressions: `2 * width + 5 mm`, `angle / 2`, `atan2(h; w)`.
//!
//! A value carries its dimension: unitless, a length (millimetres), or an
//! angle (radians). A unitless operand of `+` or `-` takes the other
//! operand's unit, in millimetres or degrees; a unitless final result takes
//! the expected unit. Trigonometric functions read a unitless argument as
//! degrees. A parameter's value is its own expression, evaluated in its
//! unit; a reference cycle has no value.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use cadmpeg_ir::math::portable::Portable;

/// The dimension of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// A pure number.
    Unitless,
    /// Millimetres.
    Length,
    /// Radians.
    Angle,
}

/// The degree in radians.
const DEGREE: f64 = PI / 180.0;
/// Deepest operator nesting of one expression.
const MAX_DEPTH: usize = 64;
/// Deepest parameter reference chain.
const MAX_REFERENCES: usize = 64;

/// Units: name, dimension, and size in millimetres or radians.
const UNITS: &[(&str, Kind, f64)] = &[
    ("mm", Kind::Length, 1.0),
    ("cm", Kind::Length, 10.0),
    ("m", Kind::Length, 1000.0),
    ("um", Kind::Length, 0.001),
    ("µm", Kind::Length, 0.001),
    ("in", Kind::Length, 25.4),
    ("inch", Kind::Length, 25.4),
    ("ft", Kind::Length, 304.8),
    ("yd", Kind::Length, 914.4),
    ("km", Kind::Length, 1_000_000.0),
    ("mil", Kind::Length, 0.0254),
    ("nm", Kind::Length, 0.000_001),
    ("deg", Kind::Angle, DEGREE),
    ("rad", Kind::Angle, 1.0),
];

/// The dimension and size of a unit name.
pub(super) fn unit(name: &str) -> Option<(Kind, f64)> {
    UNITS
        .iter()
        .find(|(unit, _, _)| *unit == name)
        .map(|(_, kind, size)| (*kind, *size))
}

#[derive(Debug, Clone, Copy)]
struct Value {
    v: f64,
    kind: Kind,
}

impl Value {
    const fn num(v: f64) -> Self {
        Self {
            v,
            kind: Kind::Unitless,
        }
    }

    /// The value in `kind`: a unitless value takes that kind's default unit.
    fn as_kind(self, kind: Kind) -> f64 {
        match (self.kind, kind) {
            (Kind::Unitless, Kind::Angle) => self.v * DEGREE,
            _ => self.v,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Operator(char),
    Open,
    Close,
    Separator,
}

fn tokens(text: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut depth = 0_usize;
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        if c.is_whitespace() {
            at += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(at + 1).is_some_and(char::is_ascii_digit))
        {
            let start = at;
            while chars
                .get(at)
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
                || (chars.get(at) == Some(&',')
                    && depth == 0
                    && chars.get(at + 1).is_some_and(char::is_ascii_digit))
            {
                at += 1;
            }
            if matches!(chars.get(at), Some('e' | 'E'))
                && chars
                    .get(at + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '-' || *c == '+')
            {
                at += 2;
                while chars.get(at).is_some_and(char::is_ascii_digit) {
                    at += 1;
                }
            }
            let literal: String = chars[start..at].iter().collect();
            out.push(Token::Number(literal.replace(',', ".").parse().ok()?));
        } else if c.is_alphabetic() || c == '_' {
            let start = at;
            while chars
                .get(at)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                at += 1;
            }
            out.push(Token::Name(chars[start..at].iter().collect()));
        } else {
            at += 1;
            out.push(match c {
                '°' => Token::Name("deg".to_owned()),
                '"' | '″' => Token::Name("in".to_owned()),
                '\'' | '′' => Token::Name("ft".to_owned()),
                '+' | '-' | '*' | '/' | '^' => Token::Operator(c),
                '×' | '·' => Token::Operator('*'),
                '÷' => Token::Operator('/'),
                '−' => Token::Operator('-'),
                '(' => {
                    depth += 1;
                    Token::Open
                }
                ')' => {
                    depth = depth.checked_sub(1)?;
                    Token::Close
                }
                ';' | ',' => Token::Separator,
                _ => return None,
            });
        }
    }
    Some(out)
}

/// A parameter: its expression and its unit's dimension.
#[derive(Debug, Clone)]
pub(super) struct Parameter {
    /// The expression text.
    pub(super) expression: String,
    /// The dimension of the parameter's unit.
    pub(super) kind: Kind,
    /// The size of the parameter's unit.
    pub(super) scale: f64,
}

/// The design's parameters, by name.
#[derive(Debug, Default)]
pub(super) struct Scope {
    parameters: BTreeMap<String, Parameter>,
}

impl Scope {
    /// The scope of the document's `params` records.
    pub(super) fn of(params: &[serde_json::Value]) -> Self {
        let mut parameters = BTreeMap::new();
        for record in params {
            let (Some(name), Some(expression)) = (
                record.get("name").and_then(serde_json::Value::as_str),
                record.get("expr").and_then(serde_json::Value::as_str),
            ) else {
                continue;
            };
            let (kind, scale) = record
                .get("unit")
                .and_then(serde_json::Value::as_str)
                .and_then(unit)
                .unwrap_or((Kind::Unitless, 1.0));
            parameters.insert(
                name.to_owned(),
                Parameter {
                    expression: expression.to_owned(),
                    kind,
                    scale,
                },
            );
        }
        Self { parameters }
    }

    /// The value of `text` as a `kind`, in millimetres or radians.
    pub(super) fn eval(&self, text: &str, kind: Kind) -> Option<f64> {
        let value = self.value(text, 0)?;
        let result = value.as_kind(kind);
        result.is_finite().then_some(result)
    }

    /// The value of a JSON number, or of a JSON string expression.
    pub(super) fn eval_json(&self, value: &serde_json::Value, kind: Kind) -> Option<f64> {
        match value {
            serde_json::Value::Number(number) => {
                number.as_f64().map(|v| Value::num(v).as_kind(kind))
            }
            serde_json::Value::String(text) => self.eval(text, kind),
            _ => None,
        }
    }

    fn value(&self, text: &str, references: usize) -> Option<Value> {
        let tokens = tokens(text)?;
        let mut parser = Parser {
            scope: self,
            tokens: &tokens,
            at: 0,
            references,
        };
        let value = parser.sum(0)?;
        (parser.at == tokens.len()).then_some(value)
    }

    fn parameter(&self, name: &str, references: usize) -> Option<Value> {
        if references >= MAX_REFERENCES {
            return None;
        }
        let parameter = self.parameters.get(name)?;
        let value = self.value(&parameter.expression, references + 1)?;
        Some(match (value.kind, parameter.kind) {
            (Kind::Unitless, Kind::Unitless) => value,
            (Kind::Unitless, kind) => Value {
                v: value.v * parameter.scale,
                kind,
            },
            _ => value,
        })
    }
}

struct Parser<'p> {
    scope: &'p Scope,
    tokens: &'p [Token],
    at: usize,
    references: usize,
}

fn combine(a: Value, b: Value) -> Option<(f64, f64, Kind)> {
    // A unitless operand of a sum takes the other operand's unit.
    Some(match (a.kind, b.kind) {
        (Kind::Unitless, other) => (a.as_kind(other), b.v, other),
        (kind, Kind::Unitless) => (a.v, b.as_kind(kind), kind),
        (left, right) if left == right => (a.v, b.v, left),
        _ => return None,
    })
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn sum(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut left = self.product(depth + 1)?;
        while let Some(Token::Operator(op @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let right = self.product(depth + 1)?;
            let (a, b, kind) = combine(left, right)?;
            left = Value {
                v: if op == '+' { a + b } else { a - b },
                kind,
            };
        }
        Some(left)
    }

    fn product(&mut self, depth: usize) -> Option<Value> {
        let mut left = self.power(depth + 1)?;
        while let Some(Token::Operator(op @ ('*' | '/'))) = self.peek().cloned() {
            self.at += 1;
            let right = self.power(depth + 1)?;
            let kind = match (left.kind, right.kind, op) {
                (kind, Kind::Unitless, _) | (Kind::Unitless, kind, '*') => kind,
                (left_kind, right_kind, '/') if left_kind == right_kind => Kind::Unitless,
                (kind, _, _) => kind,
            };
            left = Value {
                v: if op == '*' {
                    left.v * right.v
                } else {
                    left.v / right.v
                },
                kind,
            };
        }
        Some(left)
    }

    fn power(&mut self, depth: usize) -> Option<Value> {
        let base = self.unary(depth + 1)?;
        if self.peek() == Some(&Token::Operator('^')) {
            self.at += 1;
            let exponent = self.power(depth + 1)?;
            return Some(Value {
                v: base.v.powf(exponent.v),
                kind: base.kind,
            });
        }
        Some(base)
    }

    fn unary(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek() {
            Some(Token::Operator('-')) => {
                self.at += 1;
                let value = self.unary(depth + 1)?;
                Some(Value {
                    v: -value.v,
                    ..value
                })
            }
            Some(Token::Operator('+')) => {
                self.at += 1;
                self.unary(depth + 1)
            }
            _ => self.postfix(depth + 1),
        }
    }

    /// A primary and the unit names that follow it: `5 mm`, `(a + b) deg`.
    fn postfix(&mut self, depth: usize) -> Option<Value> {
        let mut value = self.primary(depth + 1)?;
        while let Some(Token::Name(name)) = self.peek() {
            let Some((kind, size)) = unit(name) else {
                break;
            };
            if value.kind != Kind::Unitless {
                break;
            }
            self.at += 1;
            value = Value {
                v: value.v * size,
                kind,
            };
        }
        Some(value)
    }

    fn primary(&mut self, depth: usize) -> Option<Value> {
        match self.peek()?.clone() {
            Token::Number(v) => {
                self.at += 1;
                Some(Value::num(v))
            }
            Token::Open => {
                self.at += 1;
                let value = self.sum(depth + 1)?;
                (self.peek() == Some(&Token::Close)).then(|| self.at += 1)?;
                Some(value)
            }
            Token::Name(name) => {
                self.at += 1;
                if self.peek() == Some(&Token::Open) {
                    self.at += 1;
                    let mut arguments = Vec::new();
                    if self.peek() != Some(&Token::Close) {
                        loop {
                            arguments.push(self.sum(depth + 1)?);
                            match self.peek()? {
                                Token::Separator => self.at += 1,
                                Token::Close => break,
                                _ => return None,
                            }
                        }
                    }
                    self.at += 1;
                    return function(&name, &arguments);
                }
                match name.as_str() {
                    "pi" | "PI" => Some(Value::num(PI)),
                    _ => match unit(&name) {
                        // A bare unit is one of it: `in` is one inch.
                        Some((kind, size)) if !self.scope.parameters.contains_key(&name) => {
                            Some(Value { v: size, kind })
                        }
                        _ => self.scope.parameter(&name, self.references),
                    },
                }
            }
            _ => None,
        }
    }
}

fn function(name: &str, arguments: &[Value]) -> Option<Value> {
    let one = || match arguments {
        [value] => Some(*value),
        _ => None,
    };
    let radians = |value: Value| value.as_kind(Kind::Angle);
    let number = Value::num;
    let angle = |v: f64| Value {
        v,
        kind: Kind::Angle,
    };
    Some(match name {
        "sin" => number(radians(one()?).portable_sin()),
        "cos" => number(radians(one()?).portable_cos()),
        "tan" => number(radians(one()?).portable_tan()),
        "asin" => angle(one()?.v.portable_asin()),
        "acos" => angle(one()?.v.portable_acos()),
        "atan" => angle(one()?.v.portable_atan()),
        "atan2" => match arguments {
            [y, x] => angle(y.v.portable_atan2(x.v)),
            _ => return None,
        },
        "sqrt" => {
            let value = one()?;
            Value {
                v: value.v.sqrt(),
                ..value
            }
        }
        "abs" => {
            let value = one()?;
            Value {
                v: value.v.abs(),
                ..value
            }
        }
        "floor" | "ceil" | "round" | "trunc" | "sign" => {
            let value = one()?;
            let v = match name {
                "floor" => value.v.floor(),
                "ceil" => value.v.ceil(),
                "round" => value.v.round(),
                "trunc" => value.v.trunc(),
                _ => value.v.signum(),
            };
            Value { v, ..value }
        }
        "min" | "max" => {
            let mut values = arguments.iter().copied();
            let first = values.next()?;
            values.try_fold(first, |best, next| {
                let (a, b, kind) = combine(best, next)?;
                let pick = if (name == "min") == (b < a) { b } else { a };
                Some(Value { v: pick, kind })
            })?
        }
        "hypot" => match arguments {
            [a, b] => {
                let (a, b, kind) = combine(*a, *b)?;
                Value {
                    v: a.hypot(b),
                    kind,
                }
            }
            _ => return None,
        },
        "pow" => match arguments {
            [x, n] => Value {
                v: x.v.powf(n.v),
                kind: x.kind,
            },
            _ => return None,
        },
        "mod" => match arguments {
            [a, b] => {
                let (a, b, kind) = combine(*a, *b)?;
                Value { v: a % b, kind }
            }
            _ => return None,
        },
        "exp" => number(one()?.v.portable_exp()),
        "ln" => number(one()?.v.portable_ln()),
        "log" | "log10" => number(one()?.v.portable_ln() / 10.0_f64.portable_ln()),
        "sinh" => number(one()?.v.portable_sinh()),
        "cosh" => number(one()?.v.portable_cosh()),
        "tanh" => number(one()?.v.portable_tanh()),
        "asinh" => number(one()?.v.portable_asinh()),
        "acosh" => number(one()?.v.portable_acosh()),
        "atanh" => number(one()?.v.portable_atanh()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope() -> Scope {
        Scope::of(&[
            json!({"name": "width", "expr": "40 mm", "unit": "mm"}),
            json!({"name": "half", "expr": "width / 2", "unit": "mm"}),
            json!({"name": "inches", "expr": "2", "unit": "in"}),
            json!({"name": "turn", "expr": "90", "unit": "deg"}),
            json!({"name": "loop", "expr": "loop + 1", "unit": "mm"}),
        ])
    }

    #[test]
    fn lengths_and_angles_take_their_units() {
        let scope = scope();
        assert_eq!(scope.eval("width / 2", Kind::Length), Some(20.0));
        assert_eq!(scope.eval("half + 5", Kind::Length), Some(25.0));
        assert_eq!(scope.eval("inches", Kind::Length), Some(50.8));
        assert_eq!(scope.eval("(4) / 2", Kind::Length), Some(2.0));
        assert_eq!(scope.eval("2 cm + 1", Kind::Length), Some(21.0));
        assert!((scope.eval("180 deg", Kind::Angle).unwrap() - PI).abs() < 1e-15);
        assert!((scope.eval("180", Kind::Angle).unwrap() - PI).abs() < 1e-15);
        assert!((scope.eval("turn / 2", Kind::Angle).unwrap() - PI / 4.0).abs() < 1e-15);
        assert!((scope.eval("atan2(1; 1)", Kind::Angle).unwrap() - PI / 4.0).abs() < 1e-15);
        assert_eq!(scope.eval("max(1; 2; 3) mm", Kind::Length), Some(3.0));
        assert_eq!(scope.eval("2 ^ 3", Kind::Length), Some(8.0));
        assert_eq!(scope.eval("1,5", Kind::Length), Some(1.5));
    }

    #[test]
    fn cycles_unknown_names_and_bad_text_have_no_value() {
        let scope = scope();
        assert_eq!(scope.eval("loop", Kind::Length), None);
        assert_eq!(scope.eval("missing * 2", Kind::Length), None);
        assert_eq!(scope.eval("(1 + 2", Kind::Length), None);
        assert_eq!(scope.eval("5 mm + 1 deg", Kind::Length), None);
        assert_eq!(scope.eval("1 $ 2", Kind::Length), None);
    }
}
