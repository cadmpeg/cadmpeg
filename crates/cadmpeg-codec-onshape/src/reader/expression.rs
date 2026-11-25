// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::document::Model;
use serde_json::Value;
use std::collections::BTreeSet;

use super::{number, Mapping};

const REFUSED: &str = "this variable expression is not supported yet";

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Dimension {
    Scalar,
    Length,
    Angle,
}

pub(crate) struct Evaluated {
    pub value: f64,
    pub expression: String,
    pub dependencies: Vec<String>,
    pub display_unit: Option<String>,
}

pub(crate) fn evaluate(
    source: &str,
    dimension: Dimension,
    model: &Model,
    unresolved: &BTreeSet<String>,
) -> Mapping<Evaluated> {
    if source.len() > 65_536 {
        return Err("the variable expression exceeds the size limit".into());
    }
    let mut parser = Parser {
        rest: source,
        model,
        unresolved,
        dependencies: Vec::new(),
        nodes: 0,
        display_unit: None,
    };
    let (value, actual, expression) = parser.sum(0)?;
    if !parser.rest.trim().is_empty() || actual != dimension || !value.is_finite() {
        return Err("the variable expression has unresolved or mismatched units; this expression is not supported yet".into());
    }
    Ok(Evaluated {
        value,
        expression,
        dependencies: parser.dependencies,
        display_unit: parser.display_unit,
    })
}

struct Parser<'a> {
    rest: &'a str,
    model: &'a Model,
    unresolved: &'a BTreeSet<String>,
    dependencies: Vec<String>,
    nodes: usize,
    display_unit: Option<String>,
}

impl Parser<'_> {
    fn take(&mut self, ch: char) -> bool {
        self.rest = self.rest.trim_start();
        if let Some(rest) = self.rest.strip_prefix(ch) {
            self.rest = rest;
            true
        } else {
            false
        }
    }
    fn sum(&mut self, depth: usize) -> Mapping<(f64, Dimension, String)> {
        let (mut a, dim, mut expression) = self.product(depth + 1)?;
        loop {
            let op = if self.take('+') {
                '+'
            } else if self.take('-') {
                '-'
            } else {
                break;
            };
            let (b, other, right) = self.product(depth + 1)?;
            if dim != other {
                return Err(REFUSED.into());
            }
            a = if op == '+' { a + b } else { a - b };
            if !a.is_finite() {
                return Err("the variable expression exceeds the finite range".into());
            }
            expression = format!("({expression} {op} {right})");
        }
        Ok((a, dim, expression))
    }
    fn product(&mut self, depth: usize) -> Mapping<(f64, Dimension, String)> {
        let (mut a, mut dim, mut expression) = self.atom(depth + 1)?;
        loop {
            let op = if self.take('*') {
                '*'
            } else if self.take('/') {
                '/'
            } else {
                break;
            };
            let (b, other, right) = self.atom(depth + 1)?;
            dim = match (op, dim, other) {
                ('*', Dimension::Scalar, d) | ('*' | '/', d, Dimension::Scalar) => d,
                ('/', a, b) if a == b => Dimension::Scalar,
                _ => return Err(REFUSED.into()),
            };
            a = if op == '*' { a * b } else { a / b };
            if !a.is_finite() {
                return Err(
                    "the variable expression divides by zero or exceeds the finite range".into(),
                );
            }
            expression = format!("({expression} {op} {right})");
        }
        Ok((a, dim, expression))
    }
    fn atom(&mut self, depth: usize) -> Mapping<(f64, Dimension, String)> {
        self.nodes += 1;
        if depth > 64 || self.nodes > super::ROWS {
            return Err("the variable expression exceeds the nesting or row limit".into());
        }
        if self.take('+') {
            return self.atom(depth + 1);
        }
        if self.take('-') {
            let (v, d, e) = self.atom(depth + 1)?;
            return Ok((-v, d, format!("(-{e})")));
        }
        if self.take('(') {
            let value = self.sum(depth + 1)?;
            if !self.take(')') {
                return Err(REFUSED.into());
            }
            return Ok(value);
        }
        if self.take('#') {
            let name = self.word()?;
            if self.unresolved.contains(&name) {
                return Err(format!("variable #{name} has an unresolved latest binding"));
            }
            let mut found = self
                .model
                .parameters
                .iter()
                .filter(|p| p.owner.is_none() && p.name == name);
            let param=found.next().ok_or("the variable is not defined before this feature; forward references are not supported yet")?;
            if found.next().is_some() {
                return Err("variable reassignment is not supported yet".into());
            }
            let wire = cadmpeg_core::serde_erase::json::to_value(param).map_err(|_| REFUSED)?;
            let value = wire.get("value").ok_or(REFUSED)?;
            let dim = match value.get("kind").and_then(Value::as_str) {
                Some("length") => Dimension::Length,
                Some("angle") => Dimension::Angle,
                Some("real") => Dimension::Scalar,
                _ => return Err(REFUSED.into()),
            };
            let id = param.id.as_str().to_owned();
            if self.display_unit.is_none() {
                self.display_unit = wire
                    .pointer("/properties/display_unit")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            if !self.dependencies.contains(&id) {
                self.dependencies.push(id);
            }
            return Ok((number(value, "value")?, dim, param.name.clone()));
        }
        self.rest = self.rest.trim_start();
        let end = self
            .rest
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_digit() || *c == '.')
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .ok_or(REFUSED)?;
        let (digits, rest) = self.rest.split_at(end);
        let mut numeric = digits.to_owned();
        self.rest = rest;
        if self.rest.starts_with(['e', 'E']) {
            let mut chars = self.rest.char_indices();
            chars.next();
            let mut size = 1;
            if let Some((_, sign)) = chars.clone().next().filter(|(_, c)| matches!(c, '+' | '-')) {
                size += sign.len_utf8();
                chars.next();
            }
            let mut count = 0;
            for (i, c) in chars {
                if !c.is_ascii_digit() {
                    break;
                }
                size = i + c.len_utf8();
                count += 1;
            }
            if count == 0 {
                return Err(REFUSED.into());
            }
            numeric.push_str(self.rest.get(..size).ok_or(REFUSED)?);
            self.rest = self.rest.get(size..).ok_or(REFUSED)?;
        }
        let value = numeric.parse::<f64>().map_err(|_| REFUSED)?;
        if !value.is_finite() {
            return Err("the variable expression exceeds the finite range".into());
        }
        self.rest = self.rest.trim_start();
        if self
            .rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            let unit = self.word()?;
            let (factor, dim, canonical) = match unit.as_str() {
                "mm" | "millimeter" => (1.0, Dimension::Length, "mm"),
                "cm" | "centimeter" => (10.0, Dimension::Length, "cm"),
                "m" | "meter" => (1000.0, Dimension::Length, "m"),
                "in" | "inch" => (25.4, Dimension::Length, "in"),
                "ft" | "foot" => (304.8, Dimension::Length, "ft"),
                "rad" | "radian" => (1.0, Dimension::Angle, "rad"),
                "deg" | "degree" => (std::f64::consts::PI / 180.0, Dimension::Angle, "deg"),
                _ => return Err(REFUSED.into()),
            };
            let value = value * factor;
            if !value.is_finite() {
                return Err("the variable expression exceeds the finite range".into());
            }
            if self.display_unit.is_none() {
                self.display_unit = Some(canonical.to_owned());
            }
            return Ok((value, dim, format!("{numeric} {canonical}")));
        }
        Ok((value, Dimension::Scalar, numeric))
    }
    fn word(&mut self) -> Mapping<String> {
        let first = self.rest.chars().next().ok_or(REFUSED)?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(REFUSED.into());
        }
        let size = self
            .rest
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .ok_or(REFUSED)?;
        let word = self.rest.get(..size).ok_or(REFUSED)?.to_owned();
        self.rest = self.rest.get(size..).ok_or(REFUSED)?;
        Ok(word)
    }
}

#[cfg(test)]
mod tests;
