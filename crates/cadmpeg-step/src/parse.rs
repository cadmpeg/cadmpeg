// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::lex::{lex, BinaryValue, LexError, Token, TokenKind};

/// One parsed Part 21 parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Reference to a DATA entity instance.
    Reference(u64),
    /// Signed integer value.
    Integer(i64),
    /// Real value.
    Real(f64),
    /// Enumeration or logical name without delimiter dots.
    Enumeration(String),
    /// Raw string-token bytes before Part 21 escape decoding.
    String(Vec<u8>),
    /// Decoded binary literal and final-byte significant-bit boundary.
    Binary(BinaryValue),
    /// Edition-3 resource value.
    Resource(String),
    /// Omitted optional value `$`.
    Omitted,
    /// Derived value `*`.
    Derived,
    /// Ordered aggregate values.
    List(Vec<Value>),
    /// Type name and its single wrapped parameter.
    Typed(String, Box<Value>),
}

/// One simple entity leaf within an entity instance.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialRecord {
    /// Uppercase entity name.
    pub name: String,
    /// Explicit external-mapping parameters.
    pub parameters: Vec<Value>,
}

/// One DATA entity instance with its exact source extent.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRecord {
    /// Numeric entity-instance name without `#`.
    pub id: u64,
    /// One leaf for a simple instance or all leaves for a complex instance.
    pub partials: Vec<PartialRecord>,
    /// Half-open byte range from instance name through semicolon.
    pub span: Range<usize>,
}

/// One entity-like record in the HEADER section.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderRecord {
    /// Header record name.
    pub name: String,
    /// Header record parameters.
    pub parameters: Vec<Value>,
}

/// One DATA section and its ordered population.
#[derive(Debug, Clone, PartialEq)]
pub struct DataSection {
    /// Edition-3 DATA section parameters.
    pub parameters: Vec<Value>,
    /// Entity-instance names in source order.
    pub records: Vec<u64>,
}

/// One edition-3 ANCHOR binding.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorEntry {
    /// Local resource name.
    pub name: String,
    /// Value bound to the resource name.
    pub value: Value,
}

/// One edition-3 external REFERENCE binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEntry {
    /// Local resource name.
    pub name: String,
    /// External resource URI.
    pub uri: String,
}

/// Parsed exchange structure and global DATA record graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    /// HEADER records in source order.
    pub header: Vec<HeaderRecord>,
    /// ANCHOR bindings in source order.
    pub anchors: Vec<AnchorEntry>,
    /// REFERENCE bindings in source order.
    pub references: Vec<ReferenceEntry>,
    /// DATA sections in source order.
    pub data: Vec<DataSection>,
    /// Complete SIGNATURE section byte range when present.
    pub signature: Option<Range<usize>>,
    /// DATA instances indexed across every DATA section.
    pub records: BTreeMap<u64, RawRecord>,
}

/// Structural or lexical exchange failure.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Tokenization failed.
    #[error(transparent)]
    Lex(#[from] LexError),
    /// Token sequence violates the exchange grammar.
    #[error("{message} at byte {offset}")]
    Syntax {
        /// Byte offset of the unexpected token or end of input.
        offset: usize,
        /// Violated grammar invariant.
        message: String,
    },
}

/// Parse one complete clear-text exchange structure and resolve DATA references.
pub fn parse(input: &[u8]) -> Result<Exchange, ParseError> {
    Parser {
        tokens: lex(input)?,
        at: 0,
        depth: 0,
    }
    .exchange()
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
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
                let id = record.id;
                if records.insert(id, record).is_some() {
                    return self.err("duplicate instance name");
                }
                ids.push(id);
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
        let anchor_bindings = anchors
            .iter()
            .map(|anchor| (anchor.name.clone(), anchor.value.clone()))
            .collect::<BTreeMap<_, _>>();
        if anchor_bindings.len() != anchors.len() {
            return self.err("duplicate anchor name");
        }
        for anchor in &mut anchors {
            anchor.value = resolve_anchor_value(&anchor.value, &anchor_bindings, &mut Vec::new())
                .map_err(|message| ParseError::Syntax { offset: 0, message })?;
        }
        for record in records.values_mut() {
            for partial in &mut record.partials {
                for value in &mut partial.parameters {
                    *value = resolve_anchor_value(value, &anchor_bindings, &mut Vec::new())
                        .map_err(|message| ParseError::Syntax {
                            offset: record.span.start,
                            message,
                        })?;
                }
            }
        }
        for anchor in &anchors {
            let mut refs = Vec::new();
            references(&anchor.value, &mut refs);
            if refs.into_iter().any(|id| !records.contains_key(&id)) {
                return self.err("unresolved instance reference in anchor binding");
            }
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
        const MAX_VALUE_DEPTH: usize = 256;
        if self.depth >= MAX_VALUE_DEPTH {
            return self.err("parameter nesting exceeds 256 levels");
        }
        self.depth += 1;
        let result = self.parameters_inner();
        self.depth -= 1;
        result
    }

    fn parameters_inner(&mut self) -> Result<Vec<Value>, ParseError> {
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

fn resolve_anchor_value(
    value: &Value,
    anchors: &BTreeMap<String, Value>,
    stack: &mut Vec<String>,
) -> Result<Value, String> {
    match value {
        Value::Resource(name) if anchors.contains_key(name) => {
            if stack.contains(name) {
                return Err(format!("cyclic anchor binding <{name}>"));
            }
            stack.push(name.clone());
            let resolved = resolve_anchor_value(&anchors[name], anchors, stack);
            stack.pop();
            resolved
        }
        Value::List(values) => values
            .iter()
            .map(|value| resolve_anchor_value(value, anchors, stack))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Value::Typed(name, value) => resolve_anchor_value(value, anchors, stack)
            .map(|value| Value::Typed(name.clone(), Box::new(value))),
        value => Ok(value.clone()),
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
