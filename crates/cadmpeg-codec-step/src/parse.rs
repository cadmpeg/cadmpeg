// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.
//!
//! The parser accepts only source deviations whose value remains unambiguous:
//! the deviation must be recoverable without guessing, observed in a real
//! producer, represented by its own diagnostic kind, and rejectable by strict
//! decode policy. Ambiguous records and duplicate names remain parse errors.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem::size_of;
use std::ops::{Index, IndexMut, Range};
use std::slice;
use std::sync::{Arc, Mutex, OnceLock};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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
    Enumeration(Box<str>),
    /// Raw string-token bytes before Part 21 escape decoding.
    String(Box<[u8]>),
    /// Decoded binary literal and final-byte significant-bit boundary.
    Binary(BinaryValue),
    /// Edition-3 resource value.
    Resource(Box<str>),
    /// Omitted optional value `$`.
    Omitted,
    /// Derived value `*`.
    Derived,
    /// Ordered aggregate values with exact retained capacity after parsing.
    List(Vec<Value>),
    /// Interned type name and its single wrapped parameter.
    Typed(Arc<str>, Box<Value>),
}

/// Parameters of one simple entity leaf.
///
/// A single parameter is stored in the leaf itself. STEP entity leaves with
/// one parameter are common, so this keeps their parameter collection off the
/// heap without changing the slice-like access used by the reader.
#[derive(Debug, Clone, PartialEq)]
pub enum Parameters {
    /// One inline parameter.
    One(Value),
    /// Zero or multiple parameters in source order with exact retained length.
    Many(Box<[Value]>),
}

impl Parameters {
    fn from_vec(mut values: Vec<Value>) -> Self {
        if values.len() == 1 {
            Self::One(values.pop().expect("length was one"))
        } else {
            Self::Many(values.into_boxed_slice())
        }
    }

    /// Number of parameters.
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Return whether this parameter collection is empty.
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Return the parameter at `index`.
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.as_slice().get(index)
    }

    /// Return the first parameter.
    pub fn first(&self) -> Option<&Value> {
        self.as_slice().first()
    }

    /// Return the last parameter.
    pub fn last(&self) -> Option<&Value> {
        self.as_slice().last()
    }

    /// Borrow parameters as a contiguous slice.
    pub fn as_slice(&self) -> &[Value] {
        match self {
            Self::One(value) => slice::from_ref(value),
            Self::Many(values) => values,
        }
    }

    /// Iterate over parameters in source order.
    pub fn iter(&self) -> slice::Iter<'_, Value> {
        self.as_slice().iter()
    }

    /// Iterate over parameters mutably in source order.
    pub fn iter_mut(&mut self) -> slice::IterMut<'_, Value> {
        match self {
            Self::One(value) => slice::from_mut(value).iter_mut(),
            Self::Many(values) => values.iter_mut(),
        }
    }
}

impl<'a> IntoIterator for &'a Parameters {
    type Item = &'a Value;
    type IntoIter = slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut Parameters {
    type Item = &'a mut Value;
    type IntoIter = slice::IterMut<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl std::ops::Index<usize> for Parameters {
    type Output = Value;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl std::ops::IndexMut<usize> for Parameters {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match self {
            Self::One(value) if index == 0 => value,
            Self::Many(values) => &mut values[index],
            Self::One(_) => panic!("parameter index out of bounds"),
        }
    }
}

/// One simple entity leaf within an entity instance.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialRecord {
    /// Uppercase entity name.
    pub name: Arc<str>,
    /// Explicit external-mapping parameters.
    pub parameters: Parameters,
}

/// Partial records with the common single-part form stored inline.
#[derive(Debug, Clone, PartialEq)]
pub enum Partials {
    /// One simple entity leaf.
    One(PartialRecord),
    /// Multiple leaves for a complex entity.
    Many(Vec<PartialRecord>),
}

impl Partials {
    fn from_vec(mut values: Vec<PartialRecord>) -> Self {
        if values.len() == 1 {
            Self::One(values.pop().expect("length was one"))
        } else {
            Self::Many(values)
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Many(values) => values.len(),
        }
    }

    pub(crate) fn first(&self) -> Option<&PartialRecord> {
        match self {
            Self::One(value) => Some(value),
            Self::Many(values) => values.first(),
        }
    }

    #[allow(
        clippy::iter_without_into_iter,
        reason = "The iterator type is crate-private because Partials is an internal parser representation."
    )]
    pub(crate) fn iter(&self) -> PartialsIter<'_> {
        match self {
            Self::One(value) => PartialsIter::One(Some(value)),
            Self::Many(values) => PartialsIter::Many(values.iter()),
        }
    }

    #[allow(
        clippy::iter_without_into_iter,
        reason = "The iterator type is crate-private because Partials is an internal parser representation."
    )]
    pub(crate) fn iter_mut(&mut self) -> PartialsIterMut<'_> {
        match self {
            Self::One(value) => PartialsIterMut::One(Some(value)),
            Self::Many(values) => PartialsIterMut::Many(values.iter_mut()),
        }
    }
}

impl Index<usize> for Partials {
    type Output = PartialRecord;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Self::One(value) if index == 0 => value,
            Self::Many(values) => &values[index],
            Self::One(_) => panic!("partial index out of bounds"),
        }
    }
}

impl IndexMut<usize> for Partials {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match self {
            Self::One(value) if index == 0 => value,
            Self::Many(values) => &mut values[index],
            Self::One(_) => panic!("partial index out of bounds"),
        }
    }
}

/// Iterator over simple or complex partial records.
pub(crate) enum PartialsIter<'a> {
    /// The inline single-part representation.
    One(Option<&'a PartialRecord>),
    /// The heap-backed complex-part representation.
    Many(slice::Iter<'a, PartialRecord>),
}

impl<'a> Iterator for PartialsIter<'a> {
    type Item = &'a PartialRecord;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(value) => value.take(),
            Self::Many(values) => values.next(),
        }
    }
}

impl DoubleEndedIterator for PartialsIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(value) => value.take(),
            Self::Many(values) => values.next_back(),
        }
    }
}

/// Mutable iterator over simple or complex partial records.
pub(crate) enum PartialsIterMut<'a> {
    /// The inline single-part representation.
    One(Option<&'a mut PartialRecord>),
    /// The heap-backed complex-part representation.
    Many(slice::IterMut<'a, PartialRecord>),
}

impl<'a> Iterator for PartialsIterMut<'a> {
    type Item = &'a mut PartialRecord;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(value) => value.take(),
            Self::Many(values) => values.next(),
        }
    }
}

impl DoubleEndedIterator for PartialsIterMut<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(value) => value.take(),
            Self::Many(values) => values.next_back(),
        }
    }
}

/// One DATA entity instance with its exact source extent.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRecord {
    /// Numeric entity-instance name without `#`.
    pub id: u64,
    /// One leaf for a simple instance or all leaves for a complex instance.
    pub partials: Partials,
    /// Half-open byte range from instance name through semicolon.
    pub span: Range<usize>,
}

/// DATA records in deterministic instance-id order.
///
/// A sorted flat table keeps each record once and uses binary search for
/// reference lookup. This avoids a second per-record index allocation while
/// keeping traversal deterministic and bounded by the source population.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordTable {
    records: Vec<RawRecord>,
}

impl RecordTable {
    fn from_sorted(records: Vec<RawRecord>) -> Self {
        debug_assert!(records.windows(2).all(|window| window[0].id < window[1].id));
        Self { records }
    }

    /// Look up one DATA record by instance id.
    pub fn get(&self, id: &u64) -> Option<&RawRecord> {
        self.records
            .binary_search_by_key(id, |record| record.id)
            .ok()
            .map(|index| &self.records[index])
    }

    pub(crate) fn values_mut(&mut self) -> slice::IterMut<'_, RawRecord> {
        self.records.iter_mut()
    }

    /// Iterate over DATA records in instance-id order.
    pub fn values(&self) -> slice::Iter<'_, RawRecord> {
        self.records.iter()
    }

    /// Iterate over instance ids and their DATA records in instance-id order.
    pub fn iter(&self) -> RecordIter<'_> {
        RecordIter {
            records: self.records.iter(),
        }
    }

    /// Test whether a DATA record with `id` exists.
    pub fn contains_key(&self, id: &u64) -> bool {
        self.get(id).is_some()
    }

    /// Number of DATA records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Return whether the DATA record table is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.records.clear();
    }
}

impl Index<&u64> for RecordTable {
    type Output = RawRecord;

    fn index(&self, id: &u64) -> &Self::Output {
        self.get(id).expect("record id must exist")
    }
}

/// Iterator over DATA records in instance-id order.
pub struct RecordIter<'a> {
    records: slice::Iter<'a, RawRecord>,
}

impl<'a> Iterator for RecordIter<'a> {
    type Item = (&'a u64, &'a RawRecord);

    fn next(&mut self) -> Option<Self::Item> {
        self.records.next().map(|record| (&record.id, record))
    }
}

impl<'a> IntoIterator for &'a RecordTable {
    type Item = (&'a u64, &'a RawRecord);
    type IntoIter = RecordIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// One entity-like record in the HEADER section.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderRecord {
    /// Header record name.
    pub name: Arc<str>,
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
    pub records: RecordTable,
    entity_ids: EntityIndex,
}

type EntityUnionCache = Mutex<HashMap<Vec<String>, Arc<[u64]>>>;

#[derive(Debug, Default)]
struct EntityIndex(
    OnceLock<HashMap<Arc<str>, Vec<u64>>>,
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
    fn entity_ids(&self) -> &HashMap<Arc<str>, Vec<u64>> {
        self.entity_ids.0.get_or_init(|| {
            let mut entity_ids = HashMap::<Arc<str>, Vec<u64>>::new();
            for (&id, record) in &self.records {
                for partial in record.partials.iter() {
                    if let Some(ids) = entity_ids.get_mut(partial.name.as_ref()) {
                        ids.push(id);
                    } else {
                        entity_ids.insert(Arc::clone(&partial.name), vec![id]);
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
                        .filter_map(|name| self.entity_ids().get(name.as_str()))
                        .map(Vec::len)
                        .sum();
                    let mut ids = Vec::with_capacity(capacity);
                    ids.extend(
                        key.iter()
                            .filter_map(|name| self.entity_ids().get(name.as_str()))
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

    /// Releases the parsed source graph after decode has extracted all data
    /// needed for semantic output and source fidelity.
    pub(crate) fn release_source_graph(&mut self) {
        self.records.clear();
        self.entity_ids = EntityIndex::default();
        self.header.clear();
        self.anchors.clear();
        self.references.clear();
        self.data.clear();
    }
}

/// Structural or lexical exchange failure.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Tokenization failed.
    #[error(transparent)]
    Lex(#[from] LexError),
    /// The caller's decode policy refused additional parser work or storage.
    #[error(transparent)]
    Resource(#[from] CodecError),
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
    parse_inner(input, None)
}

/// Parse one exchange structure while charging the caller's decode session.
pub fn parse_with_context(
    input: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<(Exchange, Vec<ParseDiagnostic>), CodecError> {
    parse_inner(input, Some(ctx)).map_err(ParseError::into_codec_error)
}

fn parse_inner(
    input: &[u8],
    budget: Option<&DecodeContext<'_>>,
) -> Result<(Exchange, Vec<ParseDiagnostic>), ParseError> {
    let lexer = Lexer::new(input);
    let mut parser = Parser {
        lexer,
        current: None,
        last_end: 0,
        depth: 0,
        diagnostics: Vec::new(),
        budget,
        name_pool: HashMap::new(),
    };
    parser.current = parser.lex_next()?;
    parser.exchange()
}

impl ParseError {
    fn into_codec_error(self) -> CodecError {
        match self {
            Self::Resource(error) => error,
            error => CodecError::Malformed(error.to_string()),
        }
    }
}

struct Parser<'input, 'ctx, 'arena> {
    lexer: Lexer<'input>,
    current: Option<Token>,
    last_end: usize,
    depth: usize,
    diagnostics: Vec<ParseDiagnostic>,
    budget: Option<&'ctx DecodeContext<'arena>>,
    name_pool: HashMap<String, Arc<str>>,
}

impl Parser<'_, '_, '_> {
    fn exchange(mut self) -> Result<(Exchange, Vec<ParseDiagnostic>), ParseError> {
        self.name("ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
        self.name("HEADER")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut header = Vec::new();
        while !self.peek_name("ENDSEC") {
            let name = self.take_name()?;
            let parameters = self.parameters()?;
            self.charge_value_vec_storage(&parameters, "step_parse_collection_storage")?;
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
                self.charge_string_storage(&name, "step_parse_name_storage")?;
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
                self.charge_string_storage(&name, "step_parse_name_storage")?;
                self.charge_string_storage(&uri, "step_parse_reference_storage")?;
                reference_entries.push(ReferenceEntry { name, uri });
            }
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut data = Vec::new();
        let mut record_values = Vec::new();
        let mut record_ids = HashSet::new();
        while self.peek_name("DATA") {
            self.next_kind()?;
            let parameters = if self.peek(&TokenKind::LParen) {
                let parameters = self.parameters()?;
                self.charge_value_vec_storage(&parameters, "step_parse_collection_storage")?;
                parameters
            } else {
                Vec::new()
            };
            self.punct(&TokenKind::Semicolon)?;
            let mut ids = Vec::new();
            while !self.peek_name("ENDSEC") {
                let record = self.record()?;
                let id = record.id;
                if !record_ids.insert(id) {
                    return self.err("duplicate instance name");
                }
                record_values.push(record);
                self.charge_retained(hash_set_entry_storage::<u64>(), "step_parse_record_index")?;
                ids.push(id);
            }
            self.name("ENDSEC")?;
            self.punct(&TokenKind::Semicolon)?;
            ids.shrink_to_fit();
            self.charge_vec_storage(&ids, "step_parse_section_storage")?;
            data.push(DataSection {
                parameters,
                records: ids,
            });
        }
        drop(record_ids);
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
        record_values.shrink_to_fit();
        self.charge_vec_storage(&record_values, "step_parse_record_table_storage")?;
        record_values.sort_unstable_by_key(|record| record.id);
        let mut records = RecordTable::from_sorted(record_values);
        if !anchors.is_empty() {
            let mut anchor_bindings = BTreeMap::new();
            for anchor in &anchors {
                self.charge_string_storage(&anchor.name, "step_anchor_binding_storage")?;
                self.charge_retained(
                    value_storage_bytes(&anchor.value),
                    "step_anchor_binding_storage",
                )?;
                self.charge_retained(
                    btree_node_storage::<String, Value>(),
                    "step_anchor_binding_storage",
                )?;
                if anchor_bindings
                    .insert(anchor.name.clone(), anchor.value.clone())
                    .is_some()
                {
                    return self.err("duplicate anchor name");
                }
            }
            let mut resolver = AnchorResolver::new(&anchor_bindings, self.budget);
            for anchor in &mut anchors {
                anchor.value = resolver
                    .resolve_root(&anchor.value)
                    .map_err(|error| error.into_parse_error(0))?;
            }
            for record in records.values_mut() {
                for partial in record.partials.iter_mut() {
                    for value in &mut partial.parameters {
                        *value = resolver
                            .resolve_root(value)
                            .map_err(|error| error.into_parse_error(record.span.start))?;
                    }
                }
            }
        }
        self.charge_vec_storage(&header, "step_parse_exchange_storage")?;
        self.charge_vec_storage(&anchors, "step_parse_exchange_storage")?;
        self.charge_vec_storage(&reference_entries, "step_parse_exchange_storage")?;
        self.charge_vec_storage(&data, "step_parse_exchange_storage")?;
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
            for partial in record.partials.iter() {
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
        self.charge_entities(1, "step_parse_record")?;
        let partials = if self.peek(&TokenKind::LParen) {
            self.next_kind()?;
            let mut parts = Vec::new();
            while !self.peek(&TokenKind::RParen) {
                parts.push(self.partial()?);
            }
            self.next_kind()?;
            parts.shrink_to_fit();
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
                    .map(|part| part.name.as_ref())
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
            Partials::from_vec(parts)
        } else {
            Partials::One(self.partial()?)
        };
        self.punct(&TokenKind::Semicolon)?;
        self.charge_partials_storage(&partials, "step_parse_record_storage")?;
        self.charge_retained(
            u64::try_from(size_of::<RawRecord>()).unwrap_or(u64::MAX),
            "step_parse_record_storage",
        )?;
        Ok(RawRecord {
            id,
            partials,
            span: start..self.previous_end(),
        })
    }

    fn partial(&mut self) -> Result<PartialRecord, ParseError> {
        let name = self.take_name()?;
        let parameters = Parameters::from_vec(self.parameters()?);
        self.charge_parameters_storage(&parameters, "step_parse_collection_storage")?;
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
        result.map(|mut values| {
            // Parser growth uses geometric capacities. Trim before a
            // collection becomes part of the retained source graph.
            values.shrink_to_fit();
            values
        })
    }

    fn parameters_inner(&mut self) -> Result<Vec<Value>, ParseError> {
        self.punct(&TokenKind::LParen)?;
        let mut values = Vec::new();
        if self.peek(&TokenKind::RParen) {
            self.next_kind()?;
            return Ok(values);
        }
        loop {
            self.charge_collection_items(1, "step_parse_parameter")?;
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
        let value = if self.peek(&TokenKind::LParen) {
            let values = self.parameters()?;
            self.charge_retained(
                allocation_bytes(values.capacity(), size_of::<Value>()),
                "step_parse_collection_storage",
            )?;
            Value::List(values)
        } else {
            match self.next_kind()? {
                TokenKind::Instance(v) => Value::Reference(v),
                TokenKind::Integer(v) => Value::Integer(v),
                TokenKind::Real(v) => Value::Real(v),
                TokenKind::Enumeration(v) => Value::Enumeration(v.into_boxed_str()),
                TokenKind::String(v) => Value::String(v.into_boxed_slice()),
                TokenKind::Binary(v) => Value::Binary(v),
                TokenKind::Resource(v) => Value::Resource(v.into_boxed_str()),
                TokenKind::Omitted => Value::Omitted,
                TokenKind::Derived => Value::Derived,
                TokenKind::Name(name) => {
                    let parameters = self.parameters()?;
                    if parameters.len() != 1 {
                        return self.err("typed parameter requires one value");
                    }
                    let name = self.intern_name(name)?;
                    Value::Typed(
                        name,
                        Box::new(
                            parameters
                                .into_iter()
                                .next()
                                .expect("parameter count was checked"),
                        ),
                    )
                }
                _ => return self.err("expected parameter value"),
            }
        };
        self.charge_retained(value_node_storage_bytes(&value), "step_parse_value_storage")?;
        Ok(value)
    }

    fn take_name(&mut self) -> Result<Arc<str>, ParseError> {
        let TokenKind::Name(name) = self.next_kind()? else {
            return self.err("expected name");
        };
        self.intern_name(name)
    }
    fn intern_name(&mut self, name: String) -> Result<Arc<str>, ParseError> {
        if let Some(interned) = self.name_pool.get(&name) {
            return Ok(Arc::clone(interned));
        }
        let interned: Arc<str> = Arc::from(name.as_str());
        self.charge_retained(
            u64::try_from(size_of::<Arc<str>>() + interned.len() + name.capacity())
                .unwrap_or(u64::MAX),
            "step_parse_name_storage",
        )?;
        self.name_pool.insert(name, Arc::clone(&interned));
        Ok(interned)
    }
    fn name(&mut self, expected: &str) -> Result<(), ParseError> {
        let actual = self.take_name()?;
        if actual.as_ref() == expected {
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
        self.current = self.lex_next()?;
        Ok(token.kind)
    }

    fn lex_next(&mut self) -> Result<Option<Token>, ParseError> {
        let token = self.lexer.next_token()?;
        if token.is_some() {
            self.charge_work(1, "step_lex_token")?;
        }
        Ok(token)
    }

    fn charge_work(&self, units: u64, operation: &'static str) -> Result<(), ParseError> {
        self.budget
            .map_or(Ok(()), |ctx| ctx.charge_work(units, operation))
            .map_err(ParseError::Resource)
    }

    fn charge_entities(&self, count: u64, operation: &'static str) -> Result<(), ParseError> {
        self.budget
            .map_or(Ok(()), |ctx| ctx.charge_entities(count, operation))
            .map_err(ParseError::Resource)
    }

    fn charge_collection_items(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), ParseError> {
        self.budget
            .map_or(Ok(()), |ctx| ctx.charge_collection_items(count, operation))
            .map_err(ParseError::Resource)
    }

    fn charge_retained(&self, bytes: u64, operation: &'static str) -> Result<(), ParseError> {
        self.budget
            .map_or(Ok(()), |ctx| ctx.charge_retained(bytes, operation, None))
            .map_err(ParseError::Resource)
    }

    fn charge_string_storage(
        &self,
        value: &String,
        operation: &'static str,
    ) -> Result<(), ParseError> {
        self.charge_retained(
            u64::try_from(value.capacity()).unwrap_or(u64::MAX),
            operation,
        )
    }

    fn charge_value_vec_storage(
        &self,
        values: &Vec<Value>,
        operation: &'static str,
    ) -> Result<(), ParseError> {
        self.charge_retained(
            allocation_bytes(values.capacity(), size_of::<Value>()),
            operation,
        )
    }

    fn charge_parameters_storage(
        &self,
        parameters: &Parameters,
        operation: &'static str,
    ) -> Result<(), ParseError> {
        if let Parameters::Many(values) = parameters {
            self.charge_retained(
                allocation_bytes(values.len(), size_of::<Value>()),
                operation,
            )?;
        }
        Ok(())
    }

    fn charge_vec_storage<T>(
        &self,
        values: &Vec<T>,
        operation: &'static str,
    ) -> Result<(), ParseError> {
        self.charge_retained(
            allocation_bytes(values.capacity(), size_of::<T>()),
            operation,
        )
    }

    fn charge_partials_storage(
        &self,
        partials: &Partials,
        operation: &'static str,
    ) -> Result<(), ParseError> {
        if let Partials::Many(values) = partials {
            self.charge_vec_storage(values, operation)?;
        }
        Ok(())
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

struct AnchorResolver<'a, 'ctx, 'arena> {
    anchors: &'a BTreeMap<String, Value>,
    memo: BTreeMap<String, (Value, usize)>,
    remaining_nodes: usize,
    budget: Option<&'ctx DecodeContext<'arena>>,
}

#[derive(Debug)]
enum AnchorResolveError {
    Syntax(String),
    Resource(CodecError),
}

impl AnchorResolveError {
    fn into_parse_error(self, offset: usize) -> ParseError {
        match self {
            Self::Syntax(message) => ParseError::Syntax { offset, message },
            Self::Resource(error) => ParseError::Resource(error),
        }
    }
}

impl<'a, 'ctx, 'arena> AnchorResolver<'a, 'ctx, 'arena> {
    const MAX_EXPANDED_NODES: usize = 1_000_000;
    const MAX_REFERENCE_DEPTH: usize = 256;

    fn new(
        anchors: &'a BTreeMap<String, Value>,
        budget: Option<&'ctx DecodeContext<'arena>>,
    ) -> Self {
        Self {
            anchors,
            memo: BTreeMap::new(),
            remaining_nodes: Self::MAX_EXPANDED_NODES,
            budget,
        }
    }

    fn resolve_root(&mut self, value: &Value) -> Result<Value, AnchorResolveError> {
        let (value, _, expanded_nodes) =
            self.resolve(value, &mut Vec::new(), self.remaining_nodes, 0)?;
        self.remaining_nodes = self
            .remaining_nodes
            .checked_sub(expanded_nodes)
            .ok_or_else(|| {
                AnchorResolveError::Syntax(
                    "aggregate expanded anchor graph exceeds 1000000 nodes".into(),
                )
            })?;
        Ok(value)
    }

    fn charge_nodes(&self, count: usize) -> Result<(), AnchorResolveError> {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        if let Some(budget) = self.budget {
            budget
                .charge_collection_items(count, "step_anchor_materialization")
                .map_err(AnchorResolveError::Resource)?;
            budget
                .charge_work(count, "step_anchor_materialization")
                .map_err(AnchorResolveError::Resource)?;
        }
        Ok(())
    }

    fn resolve(
        &mut self,
        value: &Value,
        stack: &mut Vec<String>,
        budget: usize,
        depth: usize,
    ) -> Result<(Value, usize, usize), AnchorResolveError> {
        if depth >= Self::MAX_REFERENCE_DEPTH {
            return Err(AnchorResolveError::Syntax(
                "expanded anchor graph exceeds its node or depth limit".into(),
            ));
        }
        match value {
            Value::Resource(name) if self.anchors.contains_key(name.as_ref()) => {
                if let Some((value, nodes)) = self.memo.get(name.as_ref()) {
                    if *nodes > budget {
                        return Err(AnchorResolveError::Syntax(
                            "expanded anchor value exceeds 1000000 nodes".into(),
                        ));
                    }
                    self.charge_nodes(*nodes)?;
                    let cloned = value.clone();
                    self.charge_storage(value_storage_bytes(&cloned))?;
                    return Ok((cloned, *nodes, *nodes));
                }
                if stack.iter().any(|entry| entry == name.as_ref()) {
                    return Err(AnchorResolveError::Syntax(format!(
                        "cyclic anchor binding <{name}>"
                    )));
                }
                stack.push(name.to_string());
                let source_nodes = value_node_count(&self.anchors[name.as_ref()]);
                self.charge_nodes(source_nodes)?;
                let source = self.anchors[name.as_ref()].clone();
                self.charge_storage(value_storage_bytes(&source))?;
                let resolved = self.resolve(&source, stack, budget, depth + 1);
                stack.pop();
                let (value, nodes, _) = resolved?;
                if nodes > budget {
                    return Err(AnchorResolveError::Syntax(
                        "expanded anchor value exceeds 1000000 nodes".into(),
                    ));
                }
                self.charge_nodes(nodes)?;
                self.charge_storage(value_storage_bytes(&value))?;
                self.memo.insert(name.to_string(), (value.clone(), nodes));
                self.charge_storage(value_storage_bytes(&value))?;
                Ok((value, nodes, nodes))
            }
            Value::List(values) => {
                self.charge_nodes(1)?;
                let mut nodes = 1usize;
                let mut expanded_nodes = 0usize;
                let mut resolved = Vec::with_capacity(values.len());
                for value in values {
                    let remaining = budget.checked_sub(expanded_nodes).ok_or_else(|| {
                        AnchorResolveError::Syntax(
                            "expanded anchor value exceeds 1000000 nodes".into(),
                        )
                    })?;
                    let (value, child_nodes, child_expanded_nodes) =
                        self.resolve(value, stack, remaining, depth + 1)?;
                    nodes = nodes.checked_add(child_nodes).ok_or_else(|| {
                        AnchorResolveError::Syntax(
                            "expanded anchor value exceeds 1000000 nodes".into(),
                        )
                    })?;
                    expanded_nodes = expanded_nodes
                        .checked_add(child_expanded_nodes)
                        .ok_or_else(|| {
                            AnchorResolveError::Syntax(
                                "expanded anchor value exceeds 1000000 nodes".into(),
                            )
                        })?;
                    resolved.push(value);
                }
                resolved.shrink_to_fit();
                let value = Value::List(resolved);
                self.charge_storage(value_node_storage_bytes(&value))?;
                self.charge_storage(allocation_bytes(
                    match &value {
                        Value::List(values) => values.capacity(),
                        _ => unreachable!("list value was just constructed"),
                    },
                    size_of::<Value>(),
                ))?;
                Ok((value, nodes, expanded_nodes))
            }
            Value::Typed(name, value) => {
                self.charge_nodes(1)?;
                let (value, nodes, expanded_nodes) =
                    self.resolve(value, stack, budget, depth + 1)?;
                let typed = Value::Typed(name.clone(), Box::new(value));
                self.charge_storage(value_node_storage_bytes(&typed))?;
                Ok((typed, nodes + 1, expanded_nodes))
            }
            value => {
                self.charge_nodes(1)?;
                let cloned = value.clone();
                self.charge_storage(value_node_storage_bytes(&cloned))?;
                Ok((cloned, 1, 0))
            }
        }
    }

    fn charge_storage(&self, bytes: u64) -> Result<(), AnchorResolveError> {
        if let Some(budget) = self.budget {
            budget
                .charge_retained(bytes, "step_anchor_materialization_storage", None)
                .map_err(AnchorResolveError::Resource)?;
        }
        Ok(())
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

fn value_node_count(value: &Value) -> usize {
    match value {
        Value::List(values) => values.iter().fold(1usize, |count, value| {
            count.saturating_add(value_node_count(value))
        }),
        Value::Typed(_, value) => 1usize.saturating_add(value_node_count(value)),
        _ => 1,
    }
}

fn allocation_bytes(capacity: usize, element_size: usize) -> u64 {
    u64::try_from(capacity)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::try_from(element_size).unwrap_or(u64::MAX))
}

fn btree_node_storage<K, V>() -> u64 {
    allocation_bytes(
        1,
        size_of::<(K, V)>().saturating_add(3 * size_of::<usize>()),
    )
}

fn hash_set_entry_storage<T>() -> u64 {
    allocation_bytes(1, size_of::<T>().saturating_add(2 * size_of::<usize>()))
}

fn value_node_storage_bytes(value: &Value) -> u64 {
    let dynamic = match value {
        Value::Enumeration(value) | Value::Resource(value) => value.len(),
        Value::String(value) => value.len(),
        Value::Binary(value) => value.data.len(),
        Value::Typed(_, _) => 0,
        Value::Reference(_)
        | Value::Integer(_)
        | Value::Real(_)
        | Value::Omitted
        | Value::Derived
        | Value::List(_) => 0,
    };
    u64::try_from(size_of::<Value>())
        .unwrap_or(u64::MAX)
        .saturating_add(u64::try_from(dynamic).unwrap_or(u64::MAX))
}

fn value_storage_bytes(value: &Value) -> u64 {
    value_node_storage_bytes(value).saturating_add(match value {
        Value::List(values) => allocation_bytes(values.capacity(), size_of::<Value>())
            .saturating_add(
                values
                    .iter()
                    .map(value_storage_bytes)
                    .fold(0, u64::saturating_add),
            ),
        Value::Typed(_, value) => value_storage_bytes(value),
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse, AnchorResolver, BTreeMap, Parameters, Partials, Value};

    #[test]
    fn simple_record_stores_its_partial_inline() {
        let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
        let (exchange, _) = parse(source).expect("required invariant");

        assert!(matches!(exchange.records[&1].partials, Partials::One(_)));
    }

    #[test]
    fn complex_record_retains_all_partials_in_order() {
        let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=(A()B());ENDSEC;END-ISO-10303-21;";
        let (exchange, _) = parse(source).expect("required invariant");

        let Partials::Many(partials) = &exchange.records[&1].partials else {
            panic!("complex record must retain its partial collection");
        };
        assert_eq!(partials.len(), 2);
        assert_eq!(partials[0].name.as_ref(), "A");
        assert_eq!(partials[1].name.as_ref(), "B");
    }

    #[test]
    fn record_table_sorts_ids_and_keeps_binary_lookup() {
        let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#2=A();#1=B();ENDSEC;END-ISO-10303-21;";
        let (exchange, _) = parse(source).expect("required invariant");

        let ids = exchange
            .records
            .values()
            .map(|record| record.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(
            exchange.records.get(&2).unwrap().partials[0].name.as_ref(),
            "A"
        );
        assert!(exchange.records.contains_key(&1));
    }

    #[test]
    fn parameter_storage_keeps_single_inline_and_many_exact() {
        let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=A(#2);#2=B(1,2);ENDSEC;END-ISO-10303-21;";
        let (exchange, _) = parse(source).expect("required invariant");

        assert!(matches!(
            exchange.records[&1].partials[0].parameters,
            Parameters::One(Value::Reference(2))
        ));
        let Parameters::Many(values) = &exchange.records[&2].partials[0].parameters else {
            panic!("multiple parameters must retain their collection");
        };
        assert_eq!(values.as_ref(), &[Value::Integer(1), Value::Integer(2)]);
    }

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
        let mut resolver = AnchorResolver::new(&anchors, None);
        resolver.remaining_nodes = 0;

        let ordinary = Value::List((0..1024).map(Value::Integer).collect());
        assert_eq!(resolver.resolve_root(&ordinary).ok(), Some(ordinary));
        assert_eq!(resolver.remaining_nodes, 0);
    }

    #[test]
    fn anchor_budget_still_bounds_resource_materialization() {
        let anchors = BTreeMap::from([(
            "a".to_string(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, None);
        resolver.remaining_nodes = 2;

        assert!(resolver.resolve_root(&Value::Resource("a".into())).is_err());
    }
}
