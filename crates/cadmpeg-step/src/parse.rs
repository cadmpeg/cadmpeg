// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::OnceLock;

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
    entity_ids: EntityIndex,
}

#[derive(Debug, Default)]
struct EntityIndex(OnceLock<HashMap<String, Vec<u64>>>);

impl Clone for EntityIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for EntityIndex {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Exchange {
    fn entity_ids(&self) -> &HashMap<String, Vec<u64>> {
        self.entity_ids.0.get_or_init(|| {
            let mut entity_ids = HashMap::<String, Vec<u64>>::new();
            for (&id, record) in &self.records {
                for partial in &record.partials {
                    if let Some(ids) = entity_ids.get_mut(partial.name.as_str()) {
                        ids.push(id);
                    } else {
                        entity_ids.insert(partial.name.clone(), vec![id]);
                    }
                }
            }
            entity_ids
        })
    }

    pub(crate) fn entities(&self, name: &str) -> impl Iterator<Item = (u64, &RawRecord)> {
        self.entity_ids()
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|id| self.records.get(id).map(|record| (*id, record)))
    }

    pub(crate) fn entities_any<'a>(
        &'a self,
        names: &[&str],
    ) -> impl Iterator<Item = (u64, &'a RawRecord)> {
        let mut ids = names
            .iter()
            .filter_map(|name| self.entity_ids().get(*name))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| self.records.get(&id).map(|record| (id, record)))
    }
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
        self.punct(&TokenKind::Semicolon)?;
        self.name("HEADER")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut header = Vec::new();
        while !self.peek_name("ENDSEC") {
            let name = self.take_name()?;
            let parameters = self.parameters()?;
            self.punct(&TokenKind::Semicolon)?;
            header.push(HeaderRecord { name, parameters });
        }
        self.name("ENDSEC")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut anchors = Vec::new();
        if self.peek_name("ANCHOR") {
            self.at += 1;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                let TokenKind::Resource(name) = self.next_kind()? else {
                    return self.err("expected anchor name");
                };
                self.punct(&TokenKind::Equals)?;
                let value = self.value()?;
                self.punct(&TokenKind::Semicolon)?;
                anchors.push(AnchorEntry { name, value });
            }
            self.at += 1;
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut reference_entries = Vec::new();
        if self.peek_name("REFERENCE") {
            self.at += 1;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                let TokenKind::Resource(name) = self.next_kind()? else {
                    return self.err("expected reference name");
                };
                self.punct(&TokenKind::Equals)?;
                let TokenKind::Resource(uri) = self.next_kind()? else {
                    return self.err("expected reference URI");
                };
                self.punct(&TokenKind::Semicolon)?;
                reference_entries.push(ReferenceEntry { name, uri });
            }
            self.at += 1;
            self.punct(&TokenKind::Semicolon)?;
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
            self.punct(&TokenKind::Semicolon)?;
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
            self.punct(&TokenKind::Semicolon)?;
            data.push(DataSection {
                parameters,
                records: ids,
            });
        }
        let signature = if self.peek_name("SIGNATURE") {
            let start = self.current_offset();
            self.at += 1;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                self.at += 1;
                if self.at >= self.tokens.len() {
                    return self.err("unterminated SIGNATURE section");
                }
            }
            self.at += 1;
            self.punct(&TokenKind::Semicolon)?;
            Some(start..self.previous_end())
        } else {
            None
        };
        self.name("END-ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
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
        let mut resolver = AnchorResolver::new(&anchor_bindings);
        for anchor in &mut anchors {
            anchor.value = resolver
                .resolve_root(&anchor.value)
                .map_err(|message| ParseError::Syntax { offset: 0, message })?;
        }
        for record in records.values_mut() {
            for partial in &mut record.partials {
                for value in &mut partial.parameters {
                    *value =
                        resolver
                            .resolve_root(value)
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
                return Self::err_at(record.span.start, "unresolved instance reference");
            }
        }
        Ok(Exchange {
            header,
            anchors,
            references: reference_entries,
            data,
            signature,
            records,
            entity_ids: EntityIndex::default(),
        })
    }

    fn record(&mut self) -> Result<RawRecord, ParseError> {
        let start = self.current_offset();
        let TokenKind::Instance(id) = self.next_kind()? else {
            return self.err("expected instance name");
        };
        self.punct(&TokenKind::Equals)?;
        let partials = if self.peek(&TokenKind::LParen) {
            self.at += 1;
            let mut parts = Vec::new();
            while !self.peek(&TokenKind::RParen) {
                parts.push(self.partial()?);
            }
            self.at += 1;
            if !parts.windows(2).all(|w| w[0].name < w[1].name) {
                return Self::err_at(start, "complex partial records are not alphabetical");
            }
            parts
        } else {
            vec![self.partial()?]
        };
        self.punct(&TokenKind::Semicolon)?;
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
        self.punct(&TokenKind::LParen)?;
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
        self.punct(&TokenKind::RParen)?;
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
                    Box::new(
                        parameters
                            .into_iter()
                            .next()
                            .expect("parameter count was checked"),
                    ),
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
    fn punct(&mut self, expected: &TokenKind) -> Result<(), ParseError> {
        let actual = self.next_kind()?;
        if std::mem::discriminant(&actual) == std::mem::discriminant(expected) {
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
        Self::err_at(self.current_offset(), message)
    }
    fn err_at<T>(offset: usize, message: &str) -> Result<T, ParseError> {
        Err(ParseError::Syntax {
            offset,
            message: message.into(),
        })
    }
}

struct AnchorResolver<'a> {
    anchors: &'a BTreeMap<String, Value>,
    memo: BTreeMap<String, (Value, usize)>,
    remaining_nodes: usize,
}

impl<'a> AnchorResolver<'a> {
    const MAX_EXPANDED_NODES: usize = 1_000_000;
    const MAX_REFERENCE_DEPTH: usize = 256;

    fn new(anchors: &'a BTreeMap<String, Value>) -> Self {
        Self {
            anchors,
            memo: BTreeMap::new(),
            remaining_nodes: Self::MAX_EXPANDED_NODES,
        }
    }

    fn resolve_root(&mut self, value: &Value) -> Result<Value, String> {
        let (value, _, expanded_nodes) =
            self.resolve(value, &mut Vec::new(), self.remaining_nodes, 0)?;
        self.remaining_nodes = self
            .remaining_nodes
            .checked_sub(expanded_nodes)
            .ok_or_else(|| "aggregate expanded anchor graph exceeds 1000000 nodes".to_string())?;
        Ok(value)
    }

    fn resolve(
        &mut self,
        value: &Value,
        stack: &mut Vec<String>,
        budget: usize,
        depth: usize,
    ) -> Result<(Value, usize, usize), String> {
        if depth >= Self::MAX_REFERENCE_DEPTH {
            return Err("expanded anchor graph exceeds its node or depth limit".into());
        }
        match value {
            Value::Resource(name) if self.anchors.contains_key(name) => {
                if let Some((value, nodes)) = self.memo.get(name) {
                    if *nodes > budget {
                        return Err("expanded anchor value exceeds 1000000 nodes".into());
                    }
                    return Ok((value.clone(), *nodes, *nodes));
                }
                if stack.contains(name) {
                    return Err(format!("cyclic anchor binding <{name}>"));
                }
                stack.push(name.clone());
                let source = self.anchors[name].clone();
                let resolved = self.resolve(&source, stack, budget, depth + 1);
                stack.pop();
                let (value, nodes, _) = resolved?;
                if nodes > budget {
                    return Err("expanded anchor value exceeds 1000000 nodes".into());
                }
                self.memo.insert(name.clone(), (value.clone(), nodes));
                Ok((value, nodes, nodes))
            }
            Value::List(values) => {
                let mut nodes = 1usize;
                let mut expanded_nodes = 0usize;
                let mut resolved = Vec::with_capacity(values.len());
                for value in values {
                    let remaining = budget
                        .checked_sub(expanded_nodes)
                        .ok_or_else(|| "expanded anchor value exceeds 1000000 nodes".to_string())?;
                    let (value, child_nodes, child_expanded_nodes) =
                        self.resolve(value, stack, remaining, depth + 1)?;
                    nodes = nodes
                        .checked_add(child_nodes)
                        .ok_or_else(|| "expanded anchor value exceeds 1000000 nodes".to_string())?;
                    expanded_nodes = expanded_nodes
                        .checked_add(child_expanded_nodes)
                        .ok_or_else(|| "expanded anchor value exceeds 1000000 nodes".to_string())?;
                    resolved.push(value);
                }
                Ok((Value::List(resolved), nodes, expanded_nodes))
            }
            Value::Typed(name, value) => {
                let (value, nodes, expanded_nodes) =
                    self.resolve(value, stack, budget, depth + 1)?;
                Ok((
                    Value::Typed(name.clone(), Box::new(value)),
                    nodes + 1,
                    expanded_nodes,
                ))
            }
            value => Ok((value.clone(), 1, 0)),
        }
    }
}

fn references(value: &Value, out: &mut Vec<u64>) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Reference(id) => out.push(*id),
            Value::List(values) => pending.extend(values.iter().rev()),
            Value::Typed(_, value) => pending.push(value),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, AnchorResolver, BTreeMap, Value};

    #[test]
    fn entity_index_is_not_part_of_exchange_equality() {
        let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
        let indexed = parse(source).unwrap();
        let untouched = parse(source).unwrap();
        assert_eq!(indexed.entities("POINT").count(), 1);
        assert_eq!(indexed, untouched);
    }

    #[test]
    fn anchor_budget_charges_only_resource_expansion() {
        let anchors = BTreeMap::new();
        let mut resolver = AnchorResolver::new(&anchors);
        resolver.remaining_nodes = 0;

        let ordinary = Value::List((0..1024).map(Value::Integer).collect());
        assert_eq!(resolver.resolve_root(&ordinary), Ok(ordinary));
        assert_eq!(resolver.remaining_nodes, 0);
    }

    #[test]
    fn anchor_budget_still_bounds_resource_materialization() {
        let anchors = BTreeMap::from([(
            "a".to_string(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors);
        resolver.remaining_nodes = 2;

        assert!(resolver
            .resolve_root(&Value::Resource("a".to_string()))
            .is_err());
    }
}
