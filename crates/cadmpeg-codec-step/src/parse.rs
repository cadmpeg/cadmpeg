// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.
//!
//! The parser accepts only source deviations whose value remains unambiguous:
//! the deviation must be recoverable without guessing, observed in a real
//! producer, represented by its own diagnostic kind, and rejectable by strict
//! decode policy. Ambiguous records and duplicate names remain parse errors.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

use crate::lex::{BinaryValue, LexError, Lexer, Token, TokenKind};

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

type EntityUnionCache = Mutex<HashMap<Vec<String>, Arc<[u64]>>>;

#[derive(Debug, Default)]
struct EntityIndex(
    OnceLock<HashMap<String, Vec<u64>>>,
    OnceLock<EntityUnionCache>,
);

const EMPTY_ENTITY_IDS: &[u64] = &[];

enum EntityIdIter<'a> {
    Borrowed(std::slice::Iter<'a, u64>),
    Shared { ids: Arc<[u64]>, at: usize },
}

impl Iterator for EntityIdIter<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Borrowed(ids) => ids.next().copied(),
            Self::Shared { ids, at } => {
                let id = ids.get(*at).copied();
                *at += usize::from(id.is_some());
                id
            }
        }
    }
}

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

    fn entity_unions(&self) -> &EntityUnionCache {
        self.entity_ids.1.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(crate) fn has_entity(&self, name: &str) -> bool {
        self.entity_ids().contains_key(name)
    }

    pub(crate) fn has_entity_matching(&self, matches: impl Fn(&str) -> bool) -> bool {
        self.entity_ids().keys().any(|name| matches(name))
    }

    pub(crate) fn matching_entity_ids(&self, matches: impl Fn(&str) -> bool) -> Vec<u64> {
        let mut ids = self
            .entity_ids()
            .iter()
            .filter(|(name, _)| matches(name))
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        ids
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
        let ids = if let [name] = names {
            let ids = self
                .entity_ids()
                .get(*name)
                .map_or(EMPTY_ENTITY_IDS, Vec::as_slice);
            EntityIdIter::Borrowed(ids.iter())
        } else {
            let mut key = names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>();
            key.sort_unstable();
            key.dedup();
            let mut unions = self
                .entity_unions()
                .lock()
                .expect("entity index lock poisoned");
            let ids = unions
                .entry(key.clone())
                .or_insert_with(|| {
                    let capacity = key
                        .iter()
                        .filter_map(|name| self.entity_ids().get(name))
                        .map(Vec::len)
                        .sum();
                    let mut ids = Vec::with_capacity(capacity);
                    ids.extend(
                        key.iter()
                            .filter_map(|name| self.entity_ids().get(name))
                            .flatten()
                            .copied(),
                    );
                    ids.sort_unstable();
                    ids.dedup();
                    Arc::from(ids.into_boxed_slice())
                })
                .clone();
            EntityIdIter::Shared { ids, at: 0 }
        };
        ids.filter_map(|id| self.records.get(&id).map(|record| (id, record)))
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

/// A recoverable deviation from canonical Part 21 source syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseDiagnosticKind {
    /// Complex-entity partials are not in their canonical alphabetical order.
    ComplexPartialsNotAlphabetical,
}

/// One attributable parser diagnostic that does not prevent recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    /// Byte offset of the containing complex entity instance.
    pub offset: usize,
    /// Stable diagnostic classification.
    pub kind: ParseDiagnosticKind,
    /// Human-readable explanation, including the observed and canonical order.
    pub message: String,
}

/// Parse one complete clear-text exchange structure and resolve DATA references.
pub fn parse(input: &[u8]) -> Result<(Exchange, Vec<ParseDiagnostic>), ParseError> {
    let mut lexer = Lexer::new(input);
    Parser {
        current: lexer.next_token()?,
        lexer,
        last_end: 0,
        depth: 0,
        diagnostics: Vec::new(),
    }
    .exchange()
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Option<Token>,
    last_end: usize,
    depth: usize,
    diagnostics: Vec<ParseDiagnostic>,
}

impl Parser<'_> {
    fn exchange(mut self) -> Result<(Exchange, Vec<ParseDiagnostic>), ParseError> {
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
            self.next_kind()?;
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
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut reference_entries = Vec::new();
        if self.peek_name("REFERENCE") {
            self.next_kind()?;
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
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut data = Vec::new();
        let mut records = BTreeMap::new();
        while self.peek_name("DATA") {
            self.next_kind()?;
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
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                if self.current.is_none() {
                    return self.err("unterminated SIGNATURE section");
                }
                self.next_kind()?;
            }
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            Some(start..self.previous_end())
        } else {
            None
        };
        self.name("END-ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
        if self.current.is_some() {
            return self.err("tokens after exchange terminator");
        }
        if !anchors.is_empty() {
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
        }
        let mut refs = Vec::new();
        for anchor in &anchors {
            refs.clear();
            references(&anchor.value, &mut refs);
            if refs.iter().any(|id| !records.contains_key(id)) {
                return self.err("unresolved instance reference in anchor binding");
            }
        }
        for record in records.values() {
            refs.clear();
            for partial in &record.partials {
                for value in &partial.parameters {
                    references(value, &mut refs);
                }
            }
            if refs.iter().any(|id| !records.contains_key(id)) {
                return Self::err_at(record.span.start, "unresolved instance reference");
            }
        }
        Ok((
            Exchange {
                header,
                anchors,
                references: reference_entries,
                data,
                signature,
                records,
                entity_ids: EntityIndex::default(),
            },
            self.diagnostics,
        ))
    }

    fn record(&mut self) -> Result<RawRecord, ParseError> {
        let start = self.current_offset();
        let TokenKind::Instance(id) = self.next_kind()? else {
            return self.err("expected instance name");
        };
        self.punct(&TokenKind::Equals)?;
        let partials = if self.peek(&TokenKind::LParen) {
            self.next_kind()?;
            let mut parts = Vec::new();
            while !self.peek(&TokenKind::RParen) {
                parts.push(self.partial()?);
            }
            self.next_kind()?;
            let mut canonical_names = parts
                .iter()
                .map(|part| part.name.clone())
                .collect::<Vec<_>>();
            canonical_names.sort_unstable();
            if canonical_names
                .windows(2)
                .any(|window| window[0] == window[1])
            {
                return Self::err_at(start, "duplicate complex partial name");
            }
            if !parts
                .windows(2)
                .all(|window| window[0].name < window[1].name)
            {
                let observed = parts
                    .iter()
                    .map(|part| part.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.diagnostics.push(ParseDiagnostic {
                    offset: start,
                    kind: ParseDiagnosticKind::ComplexPartialsNotAlphabetical,
                    message: format!(
                        "complex partial records are not alphabetical: observed ({observed}), expected ({})",
                        canonical_names.join(", ")
                    ),
                });
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
            self.next_kind()?;
            return Ok(values);
        }
        loop {
            values.push(self.value()?);
            if self.peek(&TokenKind::Comma) {
                self.next_kind()?;
            } else {
                break;
            }
        }
        self.punct(&TokenKind::RParen)?;
        Ok(values)
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        if self.peek(&TokenKind::LParen) {
            return Ok(Value::List(self.parameters()?));
        }
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
        self.current.as_ref().is_some_and(|token| {
            std::mem::discriminant(&token.kind) == std::mem::discriminant(expected)
        })
    }
    fn peek_name(&self, expected: &str) -> bool {
        matches!(self.current.as_ref().map(|token| &token.kind), Some(TokenKind::Name(name)) if name == expected)
    }
    fn next_kind(&mut self) -> Result<TokenKind, ParseError> {
        let Some(token) = self.current.take() else {
            return self.err("unexpected end of input");
        };
        self.last_end = token.span.end;
        self.current = self.lexer.next_token()?;
        Ok(token.kind)
    }
    fn current_offset(&self) -> usize {
        self.current
            .as_ref()
            .map_or(self.last_end, |token| token.span.start)
    }
    fn previous_end(&self) -> usize {
        self.last_end
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
        let (indexed, _) = parse(source).expect("required invariant");
        let (untouched, _) = parse(source).expect("required invariant");
        assert_eq!(indexed.entities("POINT").count(), 1);
        assert_eq!(indexed, untouched);
    }

    #[test]
    fn entity_unions_are_ordered_unique_and_name_order_independent() {
        let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#2=(A()B());#1=B();ENDSEC;END-ISO-10303-21;";
        let (exchange, _) = parse(source).expect("required invariant");

        let forward = exchange
            .entities_any(&["A", "B"])
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let reverse = exchange
            .entities_any(&["B", "A"])
            .map(|(id, _)| id)
            .collect::<Vec<_>>();

        assert_eq!(forward, vec![1, 2]);
        assert_eq!(reverse, forward);
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
