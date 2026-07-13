// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::lex::{lex, LexError, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Reference(u64),
    Integer(i64),
    Real(f64),
    Enumeration(String),
    String(Vec<u8>),
    Binary(Vec<u8>),
    Resource(String),
    Omitted,
    Derived,
    List(Vec<Value>),
    Typed(String, Box<Value>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PartialRecord {
    pub name: String,
    pub parameters: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawRecord {
    pub id: u64,
    pub partials: Vec<PartialRecord>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderRecord {
    pub name: String,
    pub parameters: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataSection {
    pub parameters: Vec<Value>,
    pub records: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnchorEntry {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEntry {
    pub name: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub header: Vec<HeaderRecord>,
    pub anchors: Vec<AnchorEntry>,
    pub references: Vec<ReferenceEntry>,
    pub data: Vec<DataSection>,
    pub signature: Option<Range<usize>>,
    pub records: BTreeMap<u64, RawRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error(transparent)]
    Lex(#[from] LexError),
    #[error("{message} at byte {offset}")]
    Syntax { offset: usize, message: String },
}

pub fn parse(input: &[u8]) -> Result<Exchange, ParseError> {
    Parser {
        tokens: lex(input)?,
        at: 0,
    }
    .exchange()
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn exchange(mut self) -> Result<Exchange, ParseError> {
        self.name("ISO-10303-21")?;
        self.punct(TokenKind::Semicolon)?;
        self.name("HEADER")?;
        self.punct(TokenKind::Semicolon)?;
        let mut header = Vec::new();
        while !self.peek_name("ENDSEC") {
            let name = self.take_name()?;
            let parameters = self.parameters()?;
            self.punct(TokenKind::Semicolon)?;
            header.push(HeaderRecord { name, parameters });
        }
        self.name("ENDSEC")?;
        self.punct(TokenKind::Semicolon)?;
        let mut anchors = Vec::new();
        if self.peek_name("ANCHOR") {
            self.at += 1;
            self.punct(TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                let name = match self.next_kind()? {
                    TokenKind::Resource(name) => name,
                    _ => return self.err("expected anchor name"),
                };
                self.punct(TokenKind::Equals)?;
                let value = self.value()?;
                self.punct(TokenKind::Semicolon)?;
                anchors.push(AnchorEntry { name, value });
            }
            self.at += 1;
            self.punct(TokenKind::Semicolon)?;
        }
        let mut reference_entries = Vec::new();
        if self.peek_name("REFERENCE") {
            self.at += 1;
            self.punct(TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                let name = match self.next_kind()? {
                    TokenKind::Resource(name) => name,
                    _ => return self.err("expected reference name"),
                };
                self.punct(TokenKind::Equals)?;
                let uri = match self.next_kind()? {
                    TokenKind::Resource(uri) => uri,
                    _ => return self.err("expected reference URI"),
                };
                self.punct(TokenKind::Semicolon)?;
                reference_entries.push(ReferenceEntry { name, uri });
            }
            self.at += 1;
            self.punct(TokenKind::Semicolon)?;
        }
        let mut data = Vec::new();
        let mut records = BTreeMap::new();
        while self.peek_name("DATA") {
            self.at += 1;
            let parameters = if self.peek(&TokenKind::LParen) {
                self.parameters()?
            } else {
                Vec::new()
            };
            self.punct(TokenKind::Semicolon)?;
            let mut ids = Vec::new();
            while !self.peek_name("ENDSEC") {
                let record = self.record()?;
                if records.insert(record.id, record.clone()).is_some() {
                    return self.err("duplicate instance name");
                }
                ids.push(record.id);
            }
            self.name("ENDSEC")?;
            self.punct(TokenKind::Semicolon)?;
            data.push(DataSection {
                parameters,
                records: ids,
            });
        }
        let signature = if self.peek_name("SIGNATURE") {
            let start = self.current_offset();
            self.at += 1;
            self.punct(TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                self.at += 1;
                if self.at >= self.tokens.len() {
                    return self.err("unterminated SIGNATURE section");
                }
            }
            self.at += 1;
            self.punct(TokenKind::Semicolon)?;
            Some(start..self.previous_end())
        } else {
            None
        };
        self.name("END-ISO-10303-21")?;
        self.punct(TokenKind::Semicolon)?;
        if self.at != self.tokens.len() {
            return self.err("tokens after exchange terminator");
        }
        for record in records.values() {
            let mut refs = Vec::new();
            for partial in &record.partials {
                for value in &partial.parameters {
                    references(value, &mut refs);
                }
            }
            if refs.into_iter().any(|id| !records.contains_key(&id)) {
                return self.err_at(record.span.start, "unresolved instance reference");
            }
        }
        Ok(Exchange {
            header,
            anchors,
            references: reference_entries,
            data,
            signature,
            records,
        })
    }

    fn record(&mut self) -> Result<RawRecord, ParseError> {
        let start = self.current_offset();
        let id = match self.next_kind()? {
            TokenKind::Instance(id) => id,
            _ => return self.err("expected instance name"),
        };
        self.punct(TokenKind::Equals)?;
        let partials = if self.peek(&TokenKind::LParen) {
            self.at += 1;
            let mut parts = Vec::new();
            while !self.peek(&TokenKind::RParen) {
                parts.push(self.partial()?);
            }
            self.at += 1;
            if !parts.windows(2).all(|w| w[0].name <= w[1].name) {
                return self.err_at(start, "complex partial records are not alphabetical");
            }
            parts
        } else {
            vec![self.partial()?]
        };
        self.punct(TokenKind::Semicolon)?;
        Ok(RawRecord {
            id,
            partials,
            span: start..self.previous_end(),
        })
    }

    fn partial(&mut self) -> Result<PartialRecord, ParseError> {
        let name = self.take_name()?;
        let parameters = self.parameters()?;
        Ok(PartialRecord { name, parameters })
    }

    fn parameters(&mut self) -> Result<Vec<Value>, ParseError> {
        self.punct(TokenKind::LParen)?;
        let mut values = Vec::new();
        if self.peek(&TokenKind::RParen) {
            self.at += 1;
            return Ok(values);
        }
        loop {
            values.push(self.value()?);
            if self.peek(&TokenKind::Comma) {
                self.at += 1;
            } else {
                break;
            }
        }
        self.punct(TokenKind::RParen)?;
        Ok(values)
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        match self.next_kind()? {
            TokenKind::Instance(v) => Ok(Value::Reference(v)),
            TokenKind::Integer(v) => Ok(Value::Integer(v)),
            TokenKind::Real(v) => Ok(Value::Real(v)),
            TokenKind::Enumeration(v) => Ok(Value::Enumeration(v)),
            TokenKind::String(v) => Ok(Value::String(v)),
            TokenKind::Binary(v) => Ok(Value::Binary(v)),
            TokenKind::Resource(v) => Ok(Value::Resource(v)),
            TokenKind::Omitted => Ok(Value::Omitted),
            TokenKind::Derived => Ok(Value::Derived),
            TokenKind::LParen => {
                self.at -= 1;
                Ok(Value::List(self.parameters()?))
            }
            TokenKind::Name(name) => {
                let parameters = self.parameters()?;
                if parameters.len() != 1 {
                    return self.err("typed parameter requires one value");
                }
                Ok(Value::Typed(
                    name,
                    Box::new(parameters.into_iter().next().unwrap()),
                ))
            }
            _ => self.err("expected parameter value"),
        }
    }

    fn take_name(&mut self) -> Result<String, ParseError> {
        match self.next_kind()? {
            TokenKind::Name(name) => Ok(name),
            _ => self.err("expected name"),
        }
    }
    fn name(&mut self, expected: &str) -> Result<(), ParseError> {
        let actual = self.take_name()?;
        if actual == expected {
            Ok(())
        } else {
            self.err(&format!("expected {expected}, found {actual}"))
        }
    }
    fn punct(&mut self, expected: TokenKind) -> Result<(), ParseError> {
        let actual = self.next_kind()?;
        if std::mem::discriminant(&actual) == std::mem::discriminant(&expected) {
            Ok(())
        } else {
            self.err("unexpected token")
        }
    }
    fn peek(&self, expected: &TokenKind) -> bool {
        self.tokens
            .get(self.at)
            .is_some_and(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(expected))
    }
    fn peek_name(&self, expected: &str) -> bool {
        matches!(self.tokens.get(self.at).map(|t| &t.kind), Some(TokenKind::Name(name)) if name == expected)
    }
    fn next_kind(&mut self) -> Result<TokenKind, ParseError> {
        let Some(token) = self.tokens.get(self.at) else {
            return self.err("unexpected end of input");
        };
        self.at += 1;
        Ok(token.kind.clone())
    }
    fn current_offset(&self) -> usize {
        self.tokens
            .get(self.at)
            .map_or_else(|| self.previous_end(), |t| t.span.start)
    }
    fn previous_end(&self) -> usize {
        self.at
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or(0, |t| t.span.end)
    }
    fn err<T>(&self, message: &str) -> Result<T, ParseError> {
        self.err_at(self.current_offset(), message)
    }
    fn err_at<T>(&self, offset: usize, message: &str) -> Result<T, ParseError> {
        Err(ParseError::Syntax {
            offset,
            message: message.into(),
        })
    }
}

fn references(value: &Value, out: &mut Vec<u64>) {
    match value {
        Value::Reference(id) => out.push(*id),
        Value::List(values) => values.iter().for_each(|v| references(v, out)),
        Value::Typed(_, value) => references(value, out),
        _ => {}
    }
}
