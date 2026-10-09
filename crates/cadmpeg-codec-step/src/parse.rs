// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.
//!
//! The parser accepts only source deviations whose value remains unambiguous:
//! the deviation must be recoverable without guessing, observed in a real
//! producer, represented by its own diagnostic kind, and rejectable by strict
//! decode policy. Ambiguous records and duplicate names remain parse errors.

use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::Arc;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

use self::implementation_level::{DeclaredImplementationLevel, ImplementationLevel};

pub(crate) mod implementation_level;

use crate::lex::{BinaryValue, LexError, Lexer, Token, TokenKind};
use crate::parse::schema_identifier::{
    split_schema_identifier, valid_schema_identifier, AdmittedSchemaIdentifier,
};

pub(crate) mod schema_identifier;

/// One parsed Part 21 parameter value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    /// Reference to a DATA entity instance.
    Reference(u64),
    /// Reference to an externally defined value instance.
    ExternalReference(u64),
    /// Reference to an EXPRESS entity constant.
    ConstantEntity(String),
    /// Reference to an EXPRESS value constant.
    ExpressValueConstant(String),
    /// Signed integer value.
    Integer(i64),
    /// Real value.
    Real(FiniteReal),
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
    /// Standard or user-defined type name and its single wrapped parameter.
    Typed(String, Box<Value>),
}

fn try_clone_value(
    value: &Value,
    budget: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Value, CodecError> {
    let _depth = budget.enter_nested("step_value_copy_depth")?;
    budget.charge_work(1, operation)?;
    Ok(match value {
        Value::Reference(id) => Value::Reference(*id),
        Value::ExternalReference(id) => Value::ExternalReference(*id),
        Value::ConstantEntity(text) => {
            Value::ConstantEntity(budget.copy_retained_text(text, operation)?)
        }
        Value::ExpressValueConstant(text) => {
            Value::ExpressValueConstant(budget.copy_retained_text(text, operation)?)
        }
        Value::Integer(value) => Value::Integer(*value),
        Value::Real(value) => Value::Real(*value),
        Value::Enumeration(text) => Value::Enumeration(budget.copy_retained_text(text, operation)?),
        Value::String(bytes) => {
            let copied = budget.copy_slice(bytes, operation)?;
            Value::String(copied)
        }
        Value::Binary(binary) => Value::Binary(binary.try_clone_for_decode(budget, operation)?),
        Value::Resource(text) => Value::Resource(budget.copy_retained_text(text, operation)?),
        Value::Omitted => Value::Omitted,
        Value::Derived => Value::Derived,
        Value::List(values) => {
            let mut copied = budget.collection_vec(values.len(), operation)?;
            let mut visited_items = (values.as_slice()).iter();
            budget.charge_work(0, "STEP try clone value value traversal")?;
            for _ in 0..visited_items.len() {
                let value = budget.next_charged(&mut visited_items, "STEP try clone value value traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                copied.push(try_clone_value(value, budget, operation)?);
            }
            Value::List(copied)
        }
        Value::Typed(name, nested) => {
            budget.charge_collection_items(1, operation)?;
            budget.charge_retained(u64_from_index(size_of::<Value>()), operation)?;
            Value::Typed(
                budget.copy_retained_text(name, operation)?,
                Box::new(try_clone_value(nested, budget, operation)?),
            )
        }
    })
}

/// One simple entity leaf within an entity instance.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PartialRecord {
    /// Uppercase standard or `!`-prefixed user-defined entity name.
    pub(crate) name: String,
    /// Explicit external-mapping parameters.
    pub(crate) parameters: Vec<Value>,
}

pub(crate) mod partials {
    use super::{DecodeContext, ParseError, PartialRecord};

    /// The nonempty partial population of one entity instance.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct RecordPartials(pub(super) Vec<PartialRecord>);

    impl RecordPartials {
        /// Builds the population of one simple entity instance.
        #[cfg(test)]
        pub(crate) fn single(first: PartialRecord) -> Self {
            Self(vec![first])
        }

        pub(super) fn single_charged(
            first: PartialRecord,
            budget: &DecodeContext<'_>,
        ) -> Result<Self, ParseError> {
            let mut records = Vec::new();
            budget.push_vec(&mut records, first, "step_parse_record_partials")?;
            Ok(Self(records))
        }

        /// The first partial record, which always exists.
        pub(crate) fn first(&self) -> &PartialRecord {
            &self.0[0]
        }
    }

    impl std::ops::Deref for RecordPartials {
        type Target = [PartialRecord];

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl std::ops::DerefMut for RecordPartials {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    impl<'a> IntoIterator for &'a RecordPartials {
        type Item = &'a PartialRecord;
        type IntoIter = std::slice::Iter<'a, PartialRecord>;

        fn into_iter(self) -> Self::IntoIter {
            self.iter()
        }
    }

    impl<'a> IntoIterator for &'a mut RecordPartials {
        type Item = &'a mut PartialRecord;
        type IntoIter = std::slice::IterMut<'a, PartialRecord>;

        fn into_iter(self) -> Self::IntoIter {
            self.iter_mut()
        }
    }
}

/// One DATA entity instance with its exact source extent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawRecord {
    /// One leaf for a simple instance or all leaves for a complex instance.
    pub(crate) partials: partials::RecordPartials,
    /// Half-open byte range from instance name through semicolon.
    pub(crate) span: Range<usize>,
}

/// One entity-like record in the HEADER section.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeaderRecord {
    /// Header record name.
    pub(crate) name: String,
    /// Header record parameters.
    pub(crate) parameters: Vec<Value>,
    /// Byte offset of the record name in the source.
    offset: usize,
}

/// One DATA section and its ordered population.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DataSection {
    /// Edition-3 DATA section parameters.
    pub(crate) parameters: Vec<Value>,
    /// Entity-instance names in source order.
    pub(crate) records: Vec<u64>,
}

/// One edition-3 ANCHOR binding.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnchorEntry {
    /// Local resource name.
    pub(crate) name: String,
    /// Value bound to the resource name.
    pub(crate) value: Value,
    /// Ordered metadata tags attached to the binding.
    tags: Vec<AnchorTag>,
}

/// One edition-3 metadata tag attached to an ANCHOR binding.
#[derive(Debug, Clone, PartialEq)]
struct AnchorTag {
    /// Tag name, preserving source case.
    name: String,
    /// Tag value.
    value: Value,
}

/// An admitted entity or value occurrence name in a REFERENCE binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReferenceName {
    Entity(u64),
    Value(u64),
}

impl cadmpeg_core::decode::cost::DecodeCost for ReferenceName {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl std::fmt::Display for ReferenceName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Entity(id) => write!(formatter, "#{id}"),
            Self::Value(id) => write!(formatter, "@{id}"),
        }
    }
}

/// One edition-3 external REFERENCE binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceEntry {
    /// External entity or value occurrence name such as `#123`.
    pub(crate) name: ReferenceName,
    /// External resource URI.
    pub(crate) uri: String,
}

/// Parsed exchange structure and global DATA record graph.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Exchange {
    /// HEADER records in source order.
    header: Vec<HeaderRecord>,
    /// ANCHOR bindings in source order.
    anchors: Vec<AnchorEntry>,
    /// REFERENCE bindings in source order.
    references: Vec<ReferenceEntry>,
    /// DATA sections in source order.
    data: Vec<DataSection>,
    /// Complete SIGNATURE section byte ranges in source order.
    signatures: Vec<Range<usize>>,
    /// DATA instances indexed across every DATA section.
    records: BTreeMap<u64, RawRecord>,
    schema_identifiers: Vec<AdmittedSchemaIdentifier>,
    implementation_level: DeclaredImplementationLevel,
    entity_ids: EntityIndex,
}

#[derive(Debug, Clone, Default)]
struct EntityIndex(Arc<BTreeMap<String, Vec<u64>>>);

impl PartialEq for EntityIndex {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// A single index list is replayed by borrow; multiple lists own their merged identifiers.
/// The owned iterator drops its backing before its reservation.
enum EntityIds<'source, 'ctx> {
    Borrowed(std::iter::Copied<std::slice::Iter<'source, u64>>),
    Scoped {
        values: std::vec::IntoIter<u64>,
        _storage: ScopedReservation<'ctx>,
    },
}

impl Iterator for EntityIds<'_, '_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Borrowed(values) => values.next(),
            Self::Scoped { values, .. } => values.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Borrowed(values) => values.size_hint(),
            Self::Scoped { values, .. } => values.size_hint(),
        }
    }
}

impl ExactSizeIterator for EntityIds<'_, '_> {}

impl EntityIndex {
    fn build(
        records: &BTreeMap<u64, RawRecord>,
        budget: &DecodeContext<'_>,
    ) -> Result<Self, ParseError> {
        let mut index = BTreeMap::<String, Vec<u64>>::new();
        let mut visited_items = (records).iter();
        budget.charge_work(0, "STEP build traversal")?;
        for _ in 0..visited_items.len() {
            let (&id, record) = budget.next_charged(&mut visited_items, "STEP build traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            let mut visited_items = (record.partials[..]).iter();
            budget.charge_work(0, "STEP build traversal")?;
            for _ in 0..visited_items.len() {
                let partial = budget.next_charged(&mut visited_items, "STEP build traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                if let Some(ids) = budget.get_mut_btree_map(
                    &mut index,
                    partial.name.as_str(),
                    "step_entity_index_lookup",
                )? {
                    budget.push_vec(ids, id, "step_entity_index_ids")?;
                } else {
                    let name = budget
                        .copy_retained_text(&partial.name, "step_entity_index_name_storage")?;
                    let ids = budget.collect_vec([id], "step_entity_index_ids")?;
                    budget.insert_btree_map(&mut index, name, ids, "step_entity_index_names")?;
                }
            }
        }
        Ok(Self(Arc::new(index)))
    }
    fn ordered_ids<'source, 'ctx>(
        lists: &[&'source [u64]],
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<EntityIds<'source, 'ctx>, CodecError> {
        ctx.charge_work(0, "STEP entity union list traversal")?;
        match lists {
            [] => return Ok(EntityIds::Borrowed([].iter().copied())),
            [ids] => return Ok(EntityIds::Borrowed(ids.iter().copied())),
            _ => {}
        }
        let mut storage = ctx.reserve_scoped(0, "STEP entity union identifier storage")?;
        let mut ids = Vec::new();
        let mut visited_items = lists.iter();
        for _ in 0..visited_items.len() {
            let list = ctx.next_charged(&mut visited_items, "STEP entity union list traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            storage.with_storage(|| {
                ctx.extend_from_slice(&mut ids, list, "STEP entity union identifier copies")
            })?;
        }
        ctx.sort_unstable_by(
            &mut ids,
            |id| id,
            Ord::cmp,
            "STEP entity union identifier sort",
        )?;
        ctx.dedup_vec(&mut ids, "STEP entity union identifier deduplication")?;
        Ok(EntityIds::Scoped {
            values: ids.into_iter(),
            _storage: storage,
        })
    }
}

impl Exchange {
    /// HEADER records admitted with the cached schema and implementation level.
    pub(crate) fn header(&self) -> &[HeaderRecord] {
        &self.header
    }

    /// Resolved ANCHOR bindings in source order.
    pub(crate) fn anchors(&self) -> &[AnchorEntry] {
        &self.anchors
    }

    /// Admitted external occurrence bindings in source order.
    pub(crate) fn references(&self) -> &[ReferenceEntry] {
        &self.references
    }

    /// Admitted DATA sections and their record populations.
    pub(crate) fn data(&self) -> &[DataSection] {
        &self.data
    }

    /// Validated SIGNATURE extents in source order.
    pub(crate) fn signatures(&self) -> &[Range<usize>] {
        &self.signatures
    }

    /// Shared record graph; mutation would invalidate the cached entity index.
    pub(crate) fn records(&self) -> &BTreeMap<u64, RawRecord> {
        &self.records
    }

    /// Header-admitted `FILE_SCHEMA` identifiers in source order.
    pub(crate) fn schema_identifiers(&self) -> impl Iterator<Item = &str> {
        self.schema_identifiers
            .iter()
            .map(AdmittedSchemaIdentifier::text)
    }

    pub(crate) fn joined_schema_identifiers(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<String, CodecError> {
        ctx.join_display_retained(
            self.schema_identifiers(),
            ",",
            "step_schema_identifier_list",
        )
    }

    /// Numeric object-identifier components for the primary schema identifier.
    pub(crate) fn primary_schema_object_identifier(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<u64>>, CodecError> {
        self.schema_identifiers
            .first()
            .map_or(Ok(None), |identifier| {
                identifier.numeric_object_identifier(ctx)
            })
    }

    /// Verbatim `FILE_DESCRIPTION` implementation-level declaration.
    pub(crate) fn implementation_level(&self) -> &str {
        self.implementation_level.text()
    }

    pub(crate) fn decode_string_with_context(
        &self,
        bytes: &[u8],
        ctx: &DecodeContext<'_>,
    ) -> Result<String, crate::strings::StringDecodeFailure> {
        crate::strings::decode_with_context(bytes, self.implementation_level.level(), ctx)
    }

    /// Release the source graph and transfer its signature extents for retention.
    pub(crate) fn release_source_graph(&mut self) -> Vec<Range<usize>> {
        self.header.clear();
        self.anchors.clear();
        self.references.clear();
        self.data.clear();
        self.records.clear();
        self.schema_identifiers.clear();
        self.entity_ids = EntityIndex::default();
        std::mem::take(&mut self.signatures)
    }

    // The record graph is immutable while its charged name index exists.
    fn entity_ids(&self) -> &BTreeMap<String, Vec<u64>> {
        &self.entity_ids.0
    }

    pub(crate) fn has_entity(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<bool, CodecError> {
        ctx.contains_key_btree_map(self.entity_ids(), name, "STEP entity name lookup")
    }

    pub(crate) fn has_entity_matching(
        &self,
        ctx: &DecodeContext<'_>,
        matches: impl Fn(&str) -> bool,
    ) -> Result<bool, CodecError> {
        ctx.any_by(
            self.entity_ids().keys(),
            |name| Ok(matches(name)),
            "STEP entity name index search",
        )
    }

    pub(crate) fn matching_entity_ids<'a>(
        &'a self,
        ctx: &'a DecodeContext<'a>,
        matches: impl Fn(&str) -> bool + 'a,
    ) -> Result<impl Iterator<Item = Result<u64, CodecError>> + 'a, CodecError> {
        let mut list_storage = ctx.reserve_scoped(0, "STEP matching entity lists")?;
        let mut lists = Vec::new();
        let mut first: Option<&[u64]> = None;
        let mut visited_items = (self.entity_ids()).iter();
        ctx.charge_work(0, "STEP matching entity name traversal")?;
        for _ in 0..visited_items.len() {
            let (name, ids) = ctx.next_charged(&mut visited_items, "STEP matching entity name traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            if matches(name) {
                if first.is_none() && lists.is_empty() {
                    first = Some(ids.as_slice());
                    continue;
                }
                if let Some(first) = first.take() {
                    ctx.push_scoped_vec(
                        &mut list_storage,
                        &mut lists,
                        first,
                        "STEP matching entity lists",
                    )?;
                }
                ctx.push_scoped_vec(
                    &mut list_storage,
                    &mut lists,
                    ids.as_slice(),
                    "STEP matching entity lists",
                )?;
            }
        }
        let mut ids = match first {
            Some(first) => EntityIndex::ordered_ids(&[first], ctx)?,
            None => EntityIndex::ordered_ids(&lists, ctx)?,
        };
        let mut failed = false;
        Ok(std::iter::from_fn(move || {
            if failed || ids.len() == 0 {
                return None;
            }
            let result = ctx
                .next_charged(&mut ids, "STEP indexed entity identifier traversal")
                .and_then(|id| {
                    id.ok_or_else(|| CodecError::malformed("entity union ended before its bound"))
                });
            failed = result.is_err();
            Some(result)
        }))
    }

    pub(crate) fn entities<'a>(
        &'a self,
        ctx: &'a DecodeContext<'_>,
        name: &str,
    ) -> Result<impl Iterator<Item = Result<(u64, &'a RawRecord), CodecError>> + 'a, CodecError>
    {
        let ids = ctx
            .get_btree_map(self.entity_ids(), name, "STEP entity name lookup")?
            .map_or(&[][..], Vec::as_slice);
        let mut ids = ids.iter();
        let mut failed = false;
        Ok(std::iter::from_fn(move || {
            if failed || ids.as_slice().is_empty() {
                return None;
            }
            let result = ctx
                .next_charged(&mut ids, "STEP indexed entity identifier traversal")
                .and_then(|id| {
                    let id = id.ok_or_else(|| {
                        CodecError::malformed("entity name index ended before its bound")
                    })?;
                    let record = ctx
                        .get_btree_map(&self.records, id, "STEP indexed entity record lookup")?
                        .ok_or_else(|| {
                            CodecError::malformed("entity name index names no record")
                        })?;
                    Ok((*id, record))
                });
            failed = result.is_err();
            Some(result)
        }))
    }

    pub(crate) fn entities_any<'a>(
        &'a self,
        ctx: &'a DecodeContext<'a>,
        names: &'a [&str],
    ) -> Result<impl Iterator<Item = Result<(u64, &'a RawRecord), CodecError>> + 'a, CodecError>
    {
        let mut list_storage = ctx.reserve_scoped(0, "STEP entity union lists")?;
        let mut lists = Vec::new();
        let mut first: Option<&[u64]> = None;
        let mut visited_items = (names).iter();
        ctx.charge_work(0, "STEP entity union name traversal")?;
        for _ in 0..visited_items.len() {
            let name = ctx.next_charged(&mut visited_items, "STEP entity union name traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            if let Some(ids) =
                ctx.get_btree_map(self.entity_ids(), *name, "STEP entity name lookup")?
            {
                if first.is_none() && lists.is_empty() {
                    first = Some(ids.as_slice());
                    continue;
                }
                if let Some(first) = first.take() {
                    ctx.push_scoped_vec(
                        &mut list_storage,
                        &mut lists,
                        first,
                        "STEP entity union lists",
                    )?;
                }
                ctx.push_scoped_vec(
                    &mut list_storage,
                    &mut lists,
                    ids.as_slice(),
                    "STEP entity union lists",
                )?;
            }
        }
        let mut ids = match first {
            Some(first) => EntityIndex::ordered_ids(&[first], ctx)?,
            None => EntityIndex::ordered_ids(&lists, ctx)?,
        };
        let mut failed = false;
        Ok(std::iter::from_fn(move || {
            if failed || ids.len() == 0 {
                return None;
            }
            let result = ctx
                .next_charged(&mut ids, "STEP indexed entity identifier traversal")
                .and_then(|id| {
                    let id = id.ok_or_else(|| {
                        CodecError::malformed("entity union ended before its bound")
                    })?;
                    let record = ctx
                        .get_btree_map(&self.records, &id, "STEP indexed entity record lookup")?
                        .ok_or_else(|| {
                            CodecError::malformed("entity name index names no record")
                        })?;
                    Ok((id, record))
                });
            failed = result.is_err();
            Some(result)
        }))
    }
}

/// Structural or lexical exchange failure.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ParseError {
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

impl From<cadmpeg_core::decode::ResourceLimit> for ParseError {
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(error.into())
    }
}

/// A recoverable deviation from canonical Part 21 source syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseDiagnosticKind {
    /// Complex-entity partials are not in their canonical alphabetical order.
    ComplexPartialsNotAlphabetical,
    /// A simple named carrier omits its inherited `name` value.
    OmittedEntityName,
    /// A `FILE_SCHEMA` object identifier has a component outside the range
    /// that its position permits.
    SchemaObjectIdentifierOutOfRange,
    /// `FILE_DESCRIPTION` declares an implementation level whose grammar is
    /// not implemented; parsing continued with the edition-3 class-3 grammar.
    ImplementationLevelUnverified,
}

/// One attributable parser diagnostic that does not prevent recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseDiagnostic {
    /// Byte offset of the containing source record.
    pub(crate) offset: usize,
    /// Stable diagnostic classification.
    pub(crate) kind: ParseDiagnosticKind,
    /// Human-readable explanation, including the observed and canonical order.
    pub(crate) message: String,
}

/// Parse a session-retained graph for storage admission tests.
#[cfg(test)]
pub(crate) fn parse_retained(
    input: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<(Exchange, Vec<ParseDiagnostic>), CodecError> {
    parse_inner(input, ctx).or_else(|error| Err(error.into_codec_error(ctx)?))
}

/// A source graph whose storage remains temporary through its use.
#[derive(Debug)]
pub(crate) struct ScopedExchange<'ctx> {
    pub(crate) exchange: Exchange,
    pub(crate) diagnostics: Vec<ParseDiagnostic>,
    _storage: ScopedReservation<'ctx>,
}

pub(crate) fn parse_with_context<'ctx>(
    input: &[u8],
    ctx: &'ctx DecodeContext<'_>,
    operation: &'static str,
) -> Result<ScopedExchange<'ctx>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let parsed = storage.with_storage(|| parse_inner(input, ctx));
    // Errors leave the graph scope in independently retained text.
    let (exchange, diagnostics) = parsed.or_else(|error| Err(error.into_codec_error(ctx)?))?;
    Ok(ScopedExchange {
        exchange,
        diagnostics,
        _storage: storage,
    })
}

pub(crate) fn parse_inner(
    input: &[u8],
    budget: &DecodeContext<'_>,
) -> Result<(Exchange, Vec<ParseDiagnostic>), ParseError> {
    let lexer = Lexer::new(input, budget);
    let mut parser = Parser {
        current: None,
        lexer,
        last_end: 0,
        depth: 0,
        diagnostics: Vec::new(),
        omitted_entity_names: None,
        budget,
    };
    parser.current = parser.lex_next()?;
    parser.exchange()
}

impl ParseError {
    fn into_codec_error(self, ctx: &DecodeContext<'_>) -> Result<CodecError, CodecError> {
        Ok(match self {
            Self::Resource(error) => error,
            Self::Lex(error) => error.into_codec_error(ctx),
            error @ Self::Syntax { .. } => CodecError::Malformed(
                ctx.format_retained(format_args!("{error}"), "STEP parse error")?,
            ),
        })
    }
}

struct Parser<'input, 'ctx, 'arena> {
    lexer: Lexer<'input, 'ctx, 'arena>,
    current: Option<Token>,
    last_end: usize,
    depth: usize,
    diagnostics: Vec<ParseDiagnostic>,
    omitted_entity_names: Option<(usize, NonZeroUsize)>,
    budget: &'ctx DecodeContext<'arena>,
}

struct HeaderAdmission {
    implementation_level: DeclaredImplementationLevel,
    schema_identifiers: Vec<AdmittedSchemaIdentifier>,
}

/// Return whether a simple geometry, topology, or representation carrier
/// carries the inherited representation-item or representation `name` before
/// its entity-specific attributes.
///
/// The list is limited to carriers handled by the STEP reader. Context,
/// representation-map, relationship, and shape-definition entities have
/// different first attributes and must keep their positional layout.
fn has_named_carrier(name: &str) -> bool {
    matches!(
        name,
        "ANNOTATION_PLANE"
            | "ANNOTATION_PLACEHOLDER_LEADER_LINE"
            | "ANNOTATION_TO_ANNOTATION_LEADER_LINE"
            | "ANNOTATION_TO_MODEL_LEADER_LINE"
            | "ADVANCED_FACE"
            | "ADVANCED_BREP_REPRESENTATION"
            | "ADVANCED_BREP_SHAPE_REPRESENTATION"
            | "APLL_POINT"
            | "APLL_POINT_WITH_SURFACE"
            | "AXIS1_PLACEMENT"
            | "AXIS2_PLACEMENT_2D"
            | "AXIS2_PLACEMENT_3D"
            | "AUXILIARY_LEADER_LINE"
            | "BEZIER_CURVE"
            | "BOUNDARY_CURVE"
            | "BREP_WITH_VOIDS"
            | "B_SPLINE_CURVE_WITH_KNOTS"
            | "B_SPLINE_SURFACE_WITH_KNOTS"
            | "CARTESIAN_POINT"
            | "CARTESIAN_TRANSFORMATION_OPERATOR_2D"
            | "CARTESIAN_TRANSFORMATION_OPERATOR_3D"
            | "CIRCLE"
            | "CLOSED_SHELL"
            | "COMPOSITE_CURVE"
            | "CONNECTED_EDGE_SET"
            | "CONNECTED_EDGE_SUB_SET"
            | "CONNECTED_FACE_SET"
            | "CONNECTED_FACE_SUB_SET"
            | "CONICAL_SURFACE"
            | "CYLINDRICAL_SURFACE"
            | "CURVE_BOUNDED_SURFACE"
            | "CURVE_REPLICA"
            | "DEFINITIONAL_REPRESENTATION"
            | "DEGENERATE_TOROIDAL_SURFACE"
            | "DIRECTION"
            | "DRAUGHTING_CALLOUT"
            | "DRAUGHTING_MODEL"
            | "EDGE_BASED_WIREFRAME_MODEL"
            | "EDGE"
            | "EDGE_CURVE"
            | "EDGE_LOOP"
            | "ELLIPSE"
            | "ELLIPTICAL_SURFACE"
            | "FACE_BASED_SURFACE_MODEL"
            | "FACE_BOUND"
            | "FACE_OUTER_BOUND"
            | "FACE_SURFACE"
            | "FACETED_BREP"
            | "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION"
            | "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION"
            | "GEOMETRIC_CURVE_SET"
            | "GEOMETRIC_SET"
            | "HYPERBOLA"
            | "INTERSECTION_CURVE"
            | "LINE"
            | "LOOP"
            | "MAPPED_ITEM"
            | "MANIFOLD_SOLID_BREP"
            | "MANIFOLD_SURFACE_SHAPE_REPRESENTATION"
            | "MEASURE_REPRESENTATION_ITEM"
            | "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"
            | "OFFSET_CURVE_2D"
            | "OFFSET_CURVE_3D"
            | "OFFSET_SURFACE"
            | "OPEN_SHELL"
            | "ORIENTED_CLOSED_SHELL"
            | "ORIENTED_EDGE"
            | "ORIENTED_FACE"
            | "ORIENTED_OPEN_SHELL"
            | "OUTER_BOUNDARY_CURVE"
            | "PARABOLA"
            | "PCURVE"
            | "PLANE"
            | "POLY_LOOP"
            | "POLYLINE"
            | "QUASI_UNIFORM_CURVE"
            | "RECTANGULAR_TRIMMED_SURFACE"
            | "REPRESENTATION"
            | "SEAM_CURVE"
            | "SEAM_EDGE"
            | "SHELL_BASED_SURFACE_MODEL"
            | "SHELL_BASED_WIREFRAME_MODEL"
            | "SHELL"
            | "SHAPE_DIMENSION_REPRESENTATION"
            | "SHAPE_REPRESENTATION"
            | "SHAPE_REPRESENTATION_WITH_PARAMETERS"
            | "SPHERICAL_SURFACE"
            | "SUBEDGE"
            | "SUBFACE"
            | "SURFACE_CURVE"
            | "SURFACE_OF_LINEAR_EXTRUSION"
            | "SURFACE_OF_REVOLUTION"
            | "SURFACE_REPLICA"
            | "TESSELLATED_FACE"
            | "TESSELLATED_CURVE_SET"
            | "TESSELLATED_GEOMETRIC_SET"
            | "REPOSITIONED_TESSELLATED_ITEM"
            | "TESSELLATED_SHELL"
            | "TESSELLATED_SOLID"
            | "TESSELLATED_SHAPE_REPRESENTATION"
            | "TOROIDAL_SURFACE"
            | "TRIMMED_CURVE"
            | "UNIFORM_CURVE"
            | "VECTOR"
            | "VERTEX"
            | "VERTEX_POINT"
            | "VERTEX_LOOP"
            | "VERTEX_SHELL"
            | "WIRE_SHELL"
    ) || (name.starts_with("ANNOTATION_") && name.ends_with("_OCCURRENCE"))
}

fn omitted_entity_name(partial: &PartialRecord) -> bool {
    has_named_carrier(&partial.name)
        && !matches!(
            partial.parameters.first(),
            Some(Value::String(_) | Value::Omitted)
        )
}

impl Parser<'_, '_, '_> {
    fn exchange(mut self) -> Result<(Exchange, Vec<ParseDiagnostic>), ParseError> {
        self.name("ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
        self.name("HEADER")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut header = Vec::new();
        while !self.peek_name("ENDSEC") {
            self.budget.charge_work(1, "STEP parser cursor traversal")?;
            let offset = self.current_offset();
            let name = self.take_name()?;
            let parameters = self.parameter_nesting(Self::parameters_inner)?;
            self.punct(&TokenKind::Semicolon)?;
            self.budget.push_vec(
                &mut header,
                HeaderRecord {
                    name,
                    parameters,
                    offset,
                },
                "step_parse_header_records",
            )?;
        }
        self.name("ENDSEC")?;
        self.punct(&TokenKind::Semicolon)?;
        let (header_admission, header_diagnostic) = match validate_header(&header, self.budget) {
            Ok(admitted) => admitted,
            Err(ValidationError::Invalid(message)) => return self.err(message),
            Err(ValidationError::Resource(error)) => return Err(ParseError::Resource(error)),
        };
        let implementation_level = header_admission.implementation_level.level();
        if let Some(diagnostic) = header_diagnostic {
            self.budget
                .push_vec(&mut self.diagnostics, diagnostic, "step_parse_diagnostics")?;
        }
        for diagnostic in schema_object_identifier_diagnostics(
            &header_admission.schema_identifiers,
            header[2].offset,
            self.budget,
        ) {
            let diagnostic = diagnostic?;
            self.budget
                .push_vec(&mut self.diagnostics, diagnostic, "step_parse_diagnostics")?;
        }
        let (schema_names_for_matching, _schema_names_storage) = self
            .budget
            .with_scoped_storage("step schema matching storage", || {
                schema_names_for_matching(&header_admission.schema_identifiers, self.budget)
            })?;
        let (header_data_references, _header_reference_storage) = match self
            .budget
            .with_scoped_storage("step header reference storage", || {
                validate_header_sections(
                    implementation_level,
                    &header,
                    &schema_names_for_matching,
                    self.budget,
                )
            }) {
            Ok(references) => references,
            Err(ValidationError::Invalid(message)) => return self.err(message),
            Err(ValidationError::Resource(error)) => return Err(ParseError::Resource(error)),
        };
        let mut anchors = Vec::new();
        if let Some(level) = implementation_level.edition3_sections_forbidden_by() {
            if self.peek_name("ANCHOR") || self.peek_name("REFERENCE") {
                let (message, _message_storage) = self.budget.format_scoped(
                    format_args!("{level} forbids ANCHOR and REFERENCE sections"),
                    "STEP forbidden section message",
                )?;
                return self.err(&message);
            }
        }
        if self.peek_name("ANCHOR") {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                self.budget.charge_work(1, "STEP parser cursor traversal")?;
                let TokenKind::Resource(name) = self.next_kind()? else {
                    return self.err("expected anchor name");
                };
                if !valid_anchor_name(self.budget, &name)? {
                    return self.err("anchor name must contain a non-digit character");
                }
                self.punct(&TokenKind::Equals)?;
                let value = self.value()?;
                if !is_anchor_item(self.budget, &value)? {
                    return self.err("invalid anchor item");
                }
                let mut tags = Vec::new();
                while self.peek(&TokenKind::LBrace) {
                    self.budget.charge_work(1, "STEP parser cursor traversal")?;
                    self.next_kind()?;
                    let TokenKind::TagName(name) = self.next_kind()? else {
                        return self.err("expected anchor tag name");
                    };
                    self.punct(&TokenKind::Colon)?;
                    let value = self.value()?;
                    if !is_anchor_item(self.budget, &value)? {
                        return self.err("invalid anchor tag item");
                    }
                    self.punct(&TokenKind::RBrace)?;
                    self.budget
                        .reserve_capacity(&mut tags, 1, "step_anchor_tag_storage")?;
                    self.budget
                        .charge_collection_items(1, "step_parse_anchor_tags")?;
                    tags.push(AnchorTag { name, value });
                }
                tags = self.budget.shrink_vec(tags, "STEP parser vector shrink")?;

                self.punct(&TokenKind::Semicolon)?;
                self.budget.push_vec(
                    &mut anchors,
                    AnchorEntry { name, value, tags },
                    "step_parse_anchors",
                )?;
            }
            self.next_kind()?;
            self.lexer.set_allow_print_controls(true);
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut reference_entries = Vec::new();
        let mut external_storage = self
            .budget
            .reserve_scoped(0, "step external identity lookup")?;
        let mut external_reference_ids = BTreeSet::new();
        let mut external_value_reference_ids = BTreeSet::new();
        if self.peek_name("REFERENCE") {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                self.budget.charge_work(1, "STEP parser cursor traversal")?;
                let (name, same_kind, other_kind, id) = match self.next_kind()? {
                    TokenKind::Instance(id) => (
                        ReferenceName::Entity(id),
                        &mut external_reference_ids,
                        &external_value_reference_ids,
                        id,
                    ),
                    TokenKind::ValueInstance(id) => (
                        ReferenceName::Value(id),
                        &mut external_value_reference_ids,
                        &external_reference_ids,
                        id,
                    ),
                    _ => return self.err("expected reference name"),
                };
                if self.budget.contains_btree_set(
                    same_kind,
                    &id,
                    "STEP external reference identity lookup",
                )? {
                    return self.err("duplicate reference name");
                }
                external_storage.with_storage(|| {
                    self.budget
                        .insert_btree_set(same_kind, id, "step_parse_external_reference_ids")
                })?;
                if self.budget.contains_btree_set(
                    other_kind,
                    &id,
                    "STEP external reference identity lookup",
                )? {
                    return self.err("duplicate external occurrence integer");
                }
                self.punct(&TokenKind::Equals)?;
                let TokenKind::Resource(uri) = self.next_kind()? else {
                    return self.err("expected reference URI");
                };
                self.punct(&TokenKind::Semicolon)?;
                self.budget.push_vec(
                    &mut reference_entries,
                    ReferenceEntry { name, uri },
                    "step_parse_reference_entries",
                )?;
            }
            self.next_kind()?;
            self.lexer.set_allow_print_controls(true);
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut data: Vec<DataSection> = Vec::new();
        let mut records = BTreeMap::new();
        let mut data_name_storage = self.budget.reserve_scoped(0, "step data name lookup")?;
        let mut data_section_names = BTreeSet::new();
        while self.peek_name("DATA") {
            self.budget.charge_work(1, "STEP parser cursor traversal")?;
            self.next_kind()?;
            if implementation_level == ImplementationLevel::LegacyEdition1 && !data.is_empty() {
                return self.err("2;1 requires one DATA section");
            }
            let parameters = if self.peek(&TokenKind::LParen) {
                if data
                    .first()
                    .is_some_and(|section| section.parameters.is_empty())
                {
                    return self.err("multiple DATA sections require section parameters");
                }
                if implementation_level == ImplementationLevel::LegacyEdition1 {
                    return self.err("2;1 forbids DATA section parameters");
                }
                let parameters = self.parameter_nesting(Self::parameters_inner)?;
                if let Err(message) = data_name_storage.with_storage(|| {
                    valid_data_parameters(
                        &parameters,
                        &schema_names_for_matching,
                        implementation_level,
                        &mut data_section_names,
                        self.budget,
                    )
                }) {
                    match message {
                        ValidationError::Invalid(message) => return self.err(message),
                        ValidationError::Resource(error) => {
                            return Err(ParseError::Resource(error))
                        }
                    }
                }
                parameters
            } else {
                if !data.is_empty() {
                    return self.err("multiple DATA sections require section parameters");
                }
                Vec::new()
            };
            self.punct(&TokenKind::Semicolon)?;
            let mut ids = Vec::new();
            while !self.peek_name("ENDSEC") {
                self.budget.charge_work(1, "STEP parser cursor traversal")?;
                let (id, record) = self.record()?;

                if self.budget.contains_key_btree_map(
                    &records,
                    &id,
                    "STEP record identity lookup",
                )? {
                    return self.err("duplicate instance name");
                }
                self.budget.insert_btree_map(
                    &mut records,
                    id,
                    record,
                    "step_parse_record_table_storage",
                )?;
                self.budget
                    .push_vec(&mut ids, id, "step_parse_section_ids")?;
            }
            self.name("ENDSEC")?;
            self.punct(&TokenKind::Semicolon)?;
            ids = self.budget.shrink_vec(ids, "STEP parser vector shrink")?;

            self.budget.push_vec(
                &mut data,
                DataSection {
                    parameters,
                    records: ids,
                },
                "step_parse_data_sections",
            )?;
        }
        if !implementation_level.is_edition3() && data.is_empty() {
            return self.err("historical implementation levels require one DATA section");
        }
        if implementation_level.is_edition3()
            && data.len() == 1
            && data[0].parameters.is_empty()
            && schema_names_for_matching.len() != 1
        {
            return self.err("an unnamed DATA section requires one FILE_SCHEMA identifier");
        }
        match validate_header_data_references(
            self.budget,
            &header_data_references,
            &data_section_names,
        ) {
            Ok(()) => {}
            Err(ValidationError::Invalid(message)) => return self.err(message),
            Err(ValidationError::Resource(error)) => return Err(ParseError::Resource(error)),
        }
        self.name("END-ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut signatures = Vec::new();
        if let Some(level) = implementation_level.edition3_sections_forbidden_by() {
            if self.peek_name("SIGNATURE") {
                let (message, _message_storage) = self.budget.format_scoped(
                    format_args!("{level} forbids SIGNATURE sections"),
                    "STEP forbidden section message",
                )?;
                return self.err(&message);
            }
        }
        while self.peek_name("SIGNATURE") {
            self.budget.charge_work(1, "STEP parser cursor traversal")?;
            let start = self.current_offset();
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            let payload_start = self.last_end;
            let payload_end = self.current_offset();
            while !self.peek_name("ENDSEC") {
                self.budget.charge_work(1, "STEP parser cursor traversal")?;
                if self.current.is_none() {
                    return self.err("unterminated SIGNATURE section");
                }
                self.next_kind()?;
            }
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            let span = start..self.previous_end();
            let payload = payload_start..payload_end;
            self.budget
                .with_scoped_storage("STEP signature validation storage", || {
                    crate::signature::decode_payload(self.lexer.input(), &payload, self.budget)
                        .map(|_| ())
                })?;
            self.budget
                .push_vec(&mut signatures, span, "step_parse_signature_spans")?;
        }
        if self.current.is_some() {
            return self.err("tokens after exchange terminator");
        }
        if self.budget.any_by(
            records.keys(),
            |id| {
                self.budget.contains_btree_set(
                    &external_reference_ids,
                    id,
                    "STEP external reference identity lookup",
                )
            },
            "STEP exchange map traversal",
        )? {
            return self.err("external reference instance collides with a DATA instance");
        }
        if self.budget.any_by(
            records.keys(),
            |id| {
                self.budget.contains_btree_set(
                    &external_value_reference_ids,
                    id,
                    "STEP external value identity lookup",
                )
            },
            "STEP exchange map traversal",
        )? {
            return self.err("external value instance collides with a DATA instance");
        }
        if !anchors.is_empty() {
            let mut binding_storage = self
                .budget
                .reserve_scoped(0, "step_anchor_binding_storage")?;
            let mut anchor_bindings = BTreeMap::new();
            let mut visited_items = (anchors[..]).iter();
            self.budget.charge_work(0, "STEP exchange traversal")?;
            for _ in 0..visited_items.len() {
                let anchor = self.budget.next_charged(&mut visited_items, "STEP exchange traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                binding_storage.with_storage(|| {
                    let name = self
                        .budget
                        .copy_retained_text(&anchor.name, "step_anchor_binding_name_copy")?;
                    let value = try_clone_value(
                        &anchor.value,
                        self.budget,
                        "step_anchor_binding_value_copy",
                    )?;
                    self.budget.insert_btree_map(
                        &mut anchor_bindings,
                        name,
                        value,
                        "step_anchor_binding_items",
                    )?;
                    Ok::<(), CodecError>(())
                })?;
            }
            if anchor_bindings.len() != anchors.len() {
                return self.err("duplicate anchor name");
            }
            let mut resolver = AnchorResolver::new(&anchor_bindings, self.budget)
                .map_err(|error| error.into_parse_error(0))?;
            let mut resolving_anchors = anchors.iter_mut();
            self.budget.charge_work(0, "STEP anchor resolution traversal")?;
            for _ in 0..resolving_anchors.len() {
                let anchor = self.budget.next_charged(&mut resolving_anchors, "STEP anchor resolution traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                anchor.value = resolver
                    .resolve_root(&anchor.value)
                    .map_err(|error| error.into_parse_error(0))?;
                let mut tags = anchor.tags.iter_mut();
                self.budget.charge_work(0, "STEP anchor tag resolution traversal")?;
                for _ in 0..tags.len() {
                    let tag = self.budget.next_charged(&mut tags, "STEP anchor tag resolution traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    tag.value = resolver
                        .resolve_root(&tag.value)
                        .map_err(|error| error.into_parse_error(0))?;
                }
            }
            let mut resolving_records = records.iter_mut();
            self.budget.charge_work(0, "STEP mutable record traversal")?;
            for _ in 0..resolving_records.len() {
                let (_, record) = self.budget.next_charged(&mut resolving_records, "STEP mutable record traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                let mut partials = record.partials.iter_mut();
                self.budget.charge_work(0, "STEP mutable partial traversal")?;
                for _ in 0..partials.len() {
                    let partial = self.budget.next_charged(&mut partials, "STEP mutable partial traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    let mut parameters = partial.parameters.iter_mut();
                    self.budget.charge_work(0, "STEP mutable parameter traversal")?;
                    for _ in 0..parameters.len() {
                        let value = self.budget.next_charged(&mut parameters, "STEP mutable parameter traversal")?
                            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                        *value = resolver
                            .resolve_root(value)
                            .map_err(|error| error.into_parse_error(record.span.start))?;
                    }
                }
            }
        }
        // Validate the source occurrence class before local REFERENCES can
        // replace a forbidden token with an ordinary value.
        let class3_restriction =
            if let Some(restriction) = implementation_level.class3_occurrence_restriction() {
                let contains = self.budget.any_by(
                    header.as_slice(),
                    |record| {
                        self.budget.any_by(
                            record.parameters.as_slice(),
                            |value| contains_class3_occurrence(self.budget, value),
                            "STEP class-3 header parameters",
                        )
                    },
                    "STEP class-3 header traversal",
                )? || self.budget.any_by(
                    anchors.as_slice(),
                    |anchor| {
                        Ok(contains_class3_occurrence(self.budget, &anchor.value)?
                            || self.budget.any_by(
                                anchor.tags.as_slice(),
                                |tag| contains_class3_occurrence(self.budget, &tag.value),
                                "STEP class-3 anchor tags",
                            )?)
                    },
                    "STEP class-3 anchor traversal",
                )? || self.budget.any_by(
                    &records,
                    |(_, record)| {
                        self.budget.any_by(
                            &record.partials[..],
                            |partial| {
                                self.budget.any_by(
                                    partial.parameters.as_slice(),
                                    |value| contains_class3_occurrence(self.budget, value),
                                    "STEP class-3 record parameters",
                                )
                            },
                            "STEP class-3 partial traversal",
                        )
                    },
                    "STEP class-3 record traversal",
                )?;
                contains.then_some(restriction)
            } else {
                None
            };
        resolve_local_references(&mut anchors, &mut records, &reference_entries, self.budget)
            .map_err(|error| error.into_parse_error(0))?;
        let mut visited_items = (records).iter_mut();
        self.budget.charge_work(0, "STEP mutable record traversal")?;
        for _ in 0..visited_items.len() {
            let (_, record) = self.budget.next_charged(&mut visited_items, "STEP mutable record traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            if record.partials.len() == 1 && omitted_entity_name(&record.partials[0]) {
                let parameters = &mut record.partials[0].parameters;

                self.budget.insert_vec(
                    parameters,
                    0,
                    Value::String(Vec::new()),
                    "step_omitted_name_recovery_item",
                )?;
                match &mut self.omitted_entity_names {
                    Some((_, count)) => {
                        *count = count.checked_add(1).ok_or_else(storage_overflow)?;
                    }
                    None => {
                        self.omitted_entity_names = Some((record.span.start, NonZeroUsize::MIN));
                    }
                }
            }
        }
        let mut reference_storage = self.budget.reserve_scoped(0, "step reference lookup")?;
        let mut refs = Vec::new();
        let mut value_refs = Vec::new();
        let mut validating_anchors = anchors.iter();
        self.budget.charge_work(0, "STEP exchange traversal")?;
        for _ in 0..validating_anchors.len() {
            let anchor = self.budget.next_charged(&mut validating_anchors, "STEP exchange traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            refs.clear();
            value_refs.clear();
            reference_storage.with_storage(|| {
                references(&anchor.value, &mut refs, &mut value_refs, self.budget)
            })?;
            if self.budget.any_by(
                &refs[..],
                |id| {
                    Ok(!self.budget.contains_key_btree_map(
                        &records,
                        id,
                        "STEP record identity lookup",
                    )? && !self.budget.contains_btree_set(
                        &external_reference_ids,
                        id,
                        "STEP external reference identity lookup",
                    )?)
                },
                "STEP exchange traversal",
            )? {
                return self.err("unresolved instance reference in anchor binding");
            }
            if self.budget.any_by(
                &value_refs[..],
                |id| {
                    Ok(!self.budget.contains_btree_set(
                        &external_value_reference_ids,
                        id,
                        "STEP external value identity lookup",
                    )?)
                },
                "STEP exchange traversal",
            )? {
                return self.err("unresolved value instance reference in anchor binding");
            }
            let mut tags = anchor.tags.iter();
            self.budget.charge_work(0, "STEP exchange traversal")?;
            for _ in 0..tags.len() {
                let tag = self.budget.next_charged(&mut tags, "STEP exchange traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                refs.clear();
                value_refs.clear();
                reference_storage.with_storage(|| {
                    references(&tag.value, &mut refs, &mut value_refs, self.budget)
                })?;
                if self.budget.any_by(
                    &refs[..],
                    |id| {
                        Ok(!self.budget.contains_key_btree_map(
                            &records,
                            id,
                            "STEP record identity lookup",
                        )? && !self.budget.contains_btree_set(
                            &external_reference_ids,
                            id,
                            "STEP external reference identity lookup",
                        )?)
                    },
                    "STEP exchange traversal",
                )? {
                    return self.err("unresolved instance reference in anchor tag");
                }
                if self.budget.any_by(
                    &value_refs[..],
                    |id| {
                        Ok(!self.budget.contains_btree_set(
                            &external_value_reference_ids,
                            id,
                            "STEP external value identity lookup",
                        )?)
                    },
                    "STEP exchange traversal",
                )? {
                    return self.err("unresolved value instance reference in anchor tag");
                }
            }
        }
        let mut validating_records = records.values();
        self.budget.charge_work(0, "STEP exchange map traversal")?;
        for _ in 0..validating_records.len() {
            let record = self.budget.next_charged(&mut validating_records, "STEP exchange map traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            refs.clear();
            value_refs.clear();
            let mut visited_items = (record.partials[..]).iter();
            self.budget.charge_work(0, "STEP exchange traversal")?;
            for _ in 0..visited_items.len() {
                let partial = self.budget.next_charged(&mut visited_items, "STEP exchange traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                let mut visited_items = (partial.parameters[..]).iter();
                self.budget.charge_work(0, "STEP exchange traversal")?;
                for _ in 0..visited_items.len() {
                    let value = self.budget.next_charged(&mut visited_items, "STEP exchange traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    reference_storage.with_storage(|| {
                        references(value, &mut refs, &mut value_refs, self.budget)
                    })?;
                }
            }
            if self.budget.any_by(
                &refs[..],
                |id| {
                    Ok(!self.budget.contains_key_btree_map(
                        &records,
                        id,
                        "STEP record identity lookup",
                    )? && !self.budget.contains_btree_set(
                        &external_reference_ids,
                        id,
                        "STEP external reference identity lookup",
                    )?)
                },
                "STEP exchange traversal",
            )? {
                return Self::err_at(
                    self.budget,
                    record.span.start,
                    "unresolved instance reference",
                );
            }
            if self.budget.any_by(
                &value_refs[..],
                |id| {
                    Ok(!self.budget.contains_btree_set(
                        &external_value_reference_ids,
                        id,
                        "STEP external value identity lookup",
                    )?)
                },
                "STEP exchange traversal",
            )? {
                return Self::err_at(
                    self.budget,
                    record.span.start,
                    "unresolved value instance reference",
                );
            }
        }
        if let Some(message) = class3_restriction {
            return self.err(message);
        }
        let has_resource_value = self.budget.any_by(
            header.as_slice(),
            |record| {
                self.budget.any_by(
                    record.parameters.as_slice(),
                    |value| contains_resource_value(self.budget, value),
                    "STEP resource header parameters",
                )
            },
            "STEP resource header traversal",
        )? || self.budget.any_by(
            &records,
            |(_, record)| {
                self.budget.any_by(
                    &record.partials[..],
                    |partial| {
                        self.budget.any_by(
                            partial.parameters.as_slice(),
                            |value| contains_resource_value(self.budget, value),
                            "STEP resource record parameters",
                        )
                    },
                    "STEP resource partial traversal",
                )
            },
            "STEP resource record traversal",
        )?;
        if has_resource_value {
            return self.err("resource values are only valid in edition-3 anchor items");
        }
        if let Some((offset, count)) = self.omitted_entity_names {
            self.budget.push_vec(&mut self.diagnostics,
                ParseDiagnostic {
                    offset,
                    kind: ParseDiagnosticKind::OmittedEntityName,
                    message: self.budget.format_retained(format_args!(
                        "recovered {count} simple named carrier instance(s) with an omitted leading name attribute by inserting an empty name"
                    ), "STEP exchange text")?,
                },
                "step_parse_diagnostics",
            )?;
        }
        header = self
            .budget
            .shrink_vec(header, "STEP parser vector shrink")?;
        anchors = self
            .budget
            .shrink_vec(anchors, "STEP parser vector shrink")?;
        reference_entries = self
            .budget
            .shrink_vec(reference_entries, "STEP parser vector shrink")?;
        data = self.budget.shrink_vec(data, "STEP parser vector shrink")?;
        signatures = self
            .budget
            .shrink_vec(signatures, "STEP parser vector shrink")?;
        let entity_ids = EntityIndex::build(&records, self.budget)?;
        Ok((
            Exchange {
                header,
                anchors,
                references: reference_entries,
                data,
                signatures,
                records,
                schema_identifiers: header_admission.schema_identifiers,
                implementation_level: header_admission.implementation_level,
                entity_ids,
            },
            self.diagnostics,
        ))
    }

    fn record(&mut self) -> Result<(u64, RawRecord), ParseError> {
        let start = self.current_offset();
        let TokenKind::Instance(id) = self.next_kind()? else {
            return self.err("expected instance name");
        };
        self.punct(&TokenKind::Equals)?;
        self.budget.charge_entities(1, "step_parse_record")?;
        let mut partials = if self.peek(&TokenKind::LParen) {
            self.next_kind()?;
            let first = self.partial()?;
            let mut parts = partials::RecordPartials::single_charged(first, self.budget)?;
            while !self.peek(&TokenKind::RParen) {
                self.budget.charge_work(1, "STEP parser cursor traversal")?;
                let partial = self.partial()?;
                self.budget
                    .push_vec(&mut parts.0, partial, "step_parse_record_partials")?;
            }
            self.next_kind()?;
            if !self.budget.all_by(
                parts.windows(2),
                |window| {
                    Ok(self.budget.compare(
                        window[0].name.as_str(),
                        window[1].name.as_str(),
                        "STEP complex partial order comparison",
                    )? == std::cmp::Ordering::Less)
                },
                "STEP complex partial order search",
            )? {
                let mut name_storage = self.budget.reserve_scoped(0, "step partial name lookup")?;
                let mut canonical_names = Vec::new();
                let mut visited_items = (parts[..]).iter();
                self.budget.charge_work(0, "STEP record traversal")?;
                for _ in 0..visited_items.len() {
                    let part = self.budget.next_charged(&mut visited_items, "STEP record traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    name_storage.with_storage(|| {
                        self.budget.push_vec(
                            &mut canonical_names,
                            part.name.as_str(),
                            "step_parse_canonical_partial_names",
                        )
                    })?;
                }
                self.budget.sort_unstable_by(
                    &mut canonical_names,
                    |value| value,
                    Ord::cmp,
                    "step_parse_canonical_partial_name_sort",
                )?;
                if self.budget.any_by(
                    canonical_names.windows(2),
                    |window| {
                        self.budget.equal(
                            window[0],
                            window[1],
                            "STEP complex partial name equality",
                        )
                    },
                    "STEP complex partial pair traversal",
                )? {
                    return Self::err_at(self.budget, start, "duplicate complex partial name");
                }
                let (observed, _observed_storage) = self.budget.with_scoped_storage(
                    "STEP observed partial name text storage",
                    || {
                        self.budget.join_display_retained(
                            parts.iter().map(|part| part.name.as_str()),
                            ", ",
                            "STEP observed partial name text",
                        )
                    },
                )?;
                let (expected, _expected_storage) = self.budget.with_scoped_storage(
                    "STEP canonical partial name text storage",
                    || {
                        self.budget.join_display_retained(
                            canonical_names.iter().copied(),
                            ", ",
                            "STEP canonical partial name text",
                        )
                    },
                )?;
                let message = self.budget.format_retained(format_args!(
                    "complex partial records are not alphabetical: observed ({observed}), expected ({expected})"
                ), "step_parse_complex_partial_diagnostic_text")?;
                self.budget.push_vec(
                    &mut self.diagnostics,
                    ParseDiagnostic {
                        offset: start,
                        kind: ParseDiagnosticKind::ComplexPartialsNotAlphabetical,
                        message,
                    },
                    "step_parse_diagnostics",
                )?;
            }
            parts
        } else {
            let first = self.partial()?;
            partials::RecordPartials::single_charged(first, self.budget)?
        };
        partials.0 = self
            .budget
            .shrink_vec(partials.0, "STEP parser vector shrink")?;
        self.punct(&TokenKind::Semicolon)?;
        Ok((
            id,
            RawRecord {
                partials,
                span: start..self.previous_end(),
            },
        ))
    }

    fn partial(&mut self) -> Result<PartialRecord, ParseError> {
        let name = self.take_name()?;
        let parameters = self.parameter_nesting(Self::parameters_inner)?;
        Ok(PartialRecord { name, parameters })
    }

    fn parameter_nesting<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        const MAX_VALUE_DEPTH: usize = 256;
        let budget = self.budget;
        let _nested = budget.enter_nested("step_parse_parameter_nesting")?;
        if self.depth >= recursion_cap(budget, MAX_VALUE_DEPTH) {
            return Err(budget
                .refuse_codec_limit(
                    "step_parse_parameter_depth_limit",
                    u64_from_index(recursion_cap(budget, MAX_VALUE_DEPTH)),
                    u64_from_index(self.depth + 1),
                )
                .into());
        }
        self.depth += 1;
        let result = parse(self);
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
            self.budget.charge_work(1, "STEP parser cursor traversal")?;
            let value = self.value()?;
            self.budget
                .push_vec(&mut values, value, "step_parse_parameter")?;
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
            Value::List(self.parameter_nesting(Self::parameters_inner)?)
        } else {
            match self.next_kind()? {
                TokenKind::Instance(v) => Value::Reference(v),
                TokenKind::ValueInstance(v) => Value::ExternalReference(v),
                TokenKind::ConstantEntity(name) => Value::ConstantEntity(name),
                TokenKind::ConstantValue(name) => Value::ExpressValueConstant(name),
                TokenKind::Integer(v) => Value::Integer(v),
                TokenKind::Real(v) => Value::Real(v),
                TokenKind::Enumeration(value) => Value::Enumeration(value),
                TokenKind::String(value) => Value::String(value),
                TokenKind::Binary(value) => Value::Binary(value),
                TokenKind::Resource(value) => Value::Resource(value),
                TokenKind::Omitted => Value::Omitted,
                TokenKind::Derived => Value::Derived,
                TokenKind::Name(name) => self.typed_parameter(name)?,
                TokenKind::UserName(name) => self.typed_parameter(
                    self.budget
                        .format_retained(format_args!("!{name}"), "step_parse_user_name_prefix")?,
                )?,
                _ => return self.err("expected parameter value"),
            }
        };
        Ok(value)
    }

    fn typed_parameter(&mut self, name: String) -> Result<Value, ParseError> {
        self.parameter_nesting(|parser| {
            parser.punct(&TokenKind::LParen)?;
            if parser.peek(&TokenKind::RParen) {
                return parser.err("typed parameter requires one value");
            }
            let value = parser.value()?;
            if parser.peek(&TokenKind::Comma) {
                return parser.err("typed parameter requires one value");
            }
            parser.punct(&TokenKind::RParen)?;
            parser
                .budget
                .charge_collection_items(1, "step_parse_typed_value")?;
            parser.budget.charge_retained(
                u64_from_index(size_of::<Value>()),
                "step_parse_typed_value_storage",
            )?;
            Ok(Value::Typed(name, Box::new(value)))
        })
    }

    fn take_name(&mut self) -> Result<String, ParseError> {
        match self.next_kind()? {
            TokenKind::Name(name) => Ok(name),
            TokenKind::UserName(name) => Ok(self
                .budget
                .format_retained(format_args!("!{name}"), "step_parse_user_name_prefix")?),
            _ => self.err("expected name"),
        }
    }
    fn name(&mut self, expected: &str) -> Result<(), ParseError> {
        let actual = self.take_name()?;
        if actual == expected {
            Ok(())
        } else {
            let message = self.budget.format_retained(
                format_args!("expected {expected}, found {actual}"),
                "step_parse_expected_name_error",
            )?;
            Err(ParseError::Syntax {
                offset: self.current_offset(),
                message,
            })
        }
    }
    fn punct(&mut self, expected: &TokenKind) -> Result<(), ParseError> {
        let actual = self.next_kind()?;
        if actual.tag() == expected.tag() {
            Ok(())
        } else {
            self.err("unexpected token")
        }
    }
    fn peek(&self, expected: &TokenKind) -> bool {
        self.current
            .as_ref()
            .is_some_and(|token| token.kind.tag() == expected.tag())
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
        Ok(self.lexer.next_token()?)
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
        Self::err_at(self.budget, self.current_offset(), message)
    }
    fn err_at<T>(ctx: &DecodeContext<'_>, offset: usize, message: &str) -> Result<T, ParseError> {
        Err(ParseError::Syntax {
            offset,
            message: ctx.copy_retained_text(message, "STEP parser error message")?,
        })
    }
}

fn storage_overflow() -> CodecError {
    cadmpeg_core::decode::refuse_local_limit("step allocation bytes", u64::MAX, u64::MAX)
}

/// Validate the three required header records, and admit the `FILE_SCHEMA`
/// identifier list.
enum ValidationError {
    Invalid(&'static str),
    Resource(CodecError),
}
impl From<cadmpeg_core::decode::ResourceLimit> for ValidationError {
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(error.into())
    }
}

impl From<CodecError> for ValidationError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl ValidationError {
    fn with_message(self, message: &'static str) -> Self {
        match self {
            Self::Invalid(_) => Self::Invalid(message),
            Self::Resource(error) => Self::Resource(error),
        }
    }
}

fn invalid<T>(message: &'static str) -> Result<T, ValidationError> {
    Err(ValidationError::Invalid(message))
}

fn validate_header(
    header: &[HeaderRecord],
    budget: &DecodeContext<'_>,
) -> Result<(HeaderAdmission, Option<ParseDiagnostic>), ValidationError> {
    const REQUIRED: [&str; 3] = ["FILE_DESCRIPTION", "FILE_NAME", "FILE_SCHEMA"];
    let [description_record, file_name_record, schema_record, ..] = header else {
        return invalid("HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA");
    };
    for (record, expected) in [description_record, file_name_record, schema_record]
        .into_iter()
        .zip(REQUIRED)
    {
        if record.name != expected {
            return invalid("HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA");
        }
    }
    if budget.any_by(
        &header[3..],
        |record| {
            Ok(matches!(
                record.name.as_str(),
                "FILE_DESCRIPTION" | "FILE_NAME" | "FILE_SCHEMA"
            ))
        },
        "STEP required header occurrence traversal",
    )? {
        return invalid("HEADER contains a duplicate required entity");
    }

    let [description_strings, implementation_level_value @ Value::String(implementation_level_bytes)] =
        description_record.parameters.as_slice()
    else {
        return invalid("FILE_DESCRIPTION has invalid parameters");
    };
    if !is_string_list(budget, Some(description_strings))? {
        return invalid("FILE_DESCRIPTION has invalid parameters");
    }
    let Some(implementation_level_text) = decoded_bytes(
        implementation_level_bytes,
        ImplementationLevel::LegacyEdition1,
        budget,
    )?
    else {
        return invalid("FILE_DESCRIPTION has an unsupported implementation level");
    };
    let declaration = DeclaredImplementationLevel::new(implementation_level_text);
    let implementation_diagnostic = if declaration.is_unverified() {
        Some(ParseDiagnostic {
            offset: description_record.offset,
            kind: ParseDiagnosticKind::ImplementationLevelUnverified,
            message: budget.format_retained(format_args!(
                    "FILE_DESCRIPTION implementation level {:?} has no implemented grammar; parsed with the 4;3 grammar",
                    declaration.text()
                ), "step_implementation_level_diagnostic_text")?,
        })
    } else {
        None
    };
    let implementation_level = declaration.level();
    if !is_decodable_string_list(Some(description_strings), implementation_level, budget)?
        || !is_decodable_string(implementation_level_value, implementation_level, budget)?
    {
        return invalid("FILE_DESCRIPTION has invalid string encoding");
    }
    if !string_list_within_limit(Some(description_strings), implementation_level, 256, budget)?
        || !string_within_limit(
            implementation_level_value,
            implementation_level,
            256,
            budget,
        )?
    {
        return invalid("FILE_DESCRIPTION contains a string longer than 256 characters");
    }

    // Producer metadata after the author and organization lists may be unset.
    let [file_name_value, file_name_timestamp, authors, organizations, preprocessor, originating_system, authorization] =
        file_name_record.parameters.as_slice()
    else {
        return invalid("FILE_NAME has invalid parameters");
    };
    if !matches!(file_name_value, Value::String(_))
        || !matches!(file_name_timestamp, Value::String(_))
        || !is_string_list(budget, Some(authors))?
        || !is_string_list(budget, Some(organizations))?
        || !is_string_or_omitted(Some(preprocessor))
        || !is_string_or_omitted(Some(originating_system))
        || !is_string_or_omitted(Some(authorization))
    {
        return invalid("FILE_NAME has invalid parameters");
    }
    let (decoded_timestamp, _timestamp_storage) = budget
        .with_scoped_storage("STEP header timestamp validation storage", || {
            decoded_string(file_name_timestamp, implementation_level, budget)
        })?;
    let Some(time_stamp) = decoded_timestamp else {
        return invalid("FILE_NAME has invalid string encoding");
    };
    if !is_decodable_string(file_name_value, implementation_level, budget)?
        || !is_decodable_string_list(Some(authors), implementation_level, budget)?
        || !is_decodable_string_list(Some(organizations), implementation_level, budget)?
        || !is_decodable_string_or_omitted(preprocessor, implementation_level, budget)?
        || !is_decodable_string_or_omitted(originating_system, implementation_level, budget)?
        || !is_decodable_string_or_omitted(authorization, implementation_level, budget)?
    {
        return invalid("FILE_NAME has invalid string encoding");
    }
    if !string_within_limit(file_name_value, implementation_level, 256, budget)?
        || !string_within_limit(file_name_timestamp, implementation_level, 256, budget)?
        || !string_list_within_limit(Some(authors), implementation_level, 256, budget)?
        || !string_list_within_limit(Some(organizations), implementation_level, 256, budget)?
        || !string_or_omitted_within_limit(preprocessor, implementation_level, 256, budget)?
        || !string_or_omitted_within_limit(originating_system, implementation_level, 256, budget)?
        || !string_or_omitted_within_limit(authorization, implementation_level, 256, budget)?
    {
        return invalid("FILE_NAME contains a string longer than 256 characters");
    }
    if !time_stamp.is_empty() && !valid_timestamp_text(budget, &time_stamp)? {
        return invalid("FILE_NAME has an invalid timestamp");
    }

    let schema = &schema_record.parameters;
    let Some(Value::List(identifiers)) = schema.first() else {
        return invalid("FILE_SCHEMA must contain one schema identifier list");
    };
    if schema.len() != 1 || identifiers.is_empty() {
        return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
    }
    let mut admitted = Vec::new();
    let mut normalized_storage =
        budget.reserve_scoped(0, "STEP schema identifier uniqueness storage")?;
    let mut normalized_identifiers = BTreeSet::new();
    let mut identifiers = identifiers.iter();
    budget.charge_work(0, "STEP schema identifier validation traversal")?;
    for _ in 0..identifiers.len() {
        let value = budget.next_charged(&mut identifiers, "STEP schema identifier validation traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        let Value::String(bytes) = value else {
            return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
        };
        let Some(identifier) = decoded_bytes(bytes, implementation_level, budget)? else {
            return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
        };
        let trimmed =
            budget.trim_text(identifier.as_str(), "STEP declared schema identifier trim")?;
        let inserted = normalized_storage.with_storage(|| {
            let normalized =
                budget.to_ascii_uppercase(trimmed, "step_schema_identifier_normalized")?;
            budget.insert_btree_set(
                &mut normalized_identifiers,
                normalized,
                "step_schema_identifier_names",
            )
        })?;
        if !inserted {
            return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
        }
        let Some(identifier) = AdmittedSchemaIdentifier::admit(budget, identifier)? else {
            return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
        };
        budget
            .push_vec(&mut admitted, identifier, "step_schema_identifiers")
            .map_err(ValidationError::Resource)?;
    }
    Ok((
        HeaderAdmission {
            implementation_level: declaration,
            schema_identifiers: admitted,
        },
        implementation_diagnostic,
    ))
}

/// One diagnostic for each `FILE_SCHEMA` identifier that the header admits
/// under its schema name alone.
fn schema_object_identifier_diagnostics<'a>(
    admitted: &'a [AdmittedSchemaIdentifier],
    offset: usize,
    budget: &'a DecodeContext<'_>,
) -> impl Iterator<Item = Result<ParseDiagnostic, CodecError>> + 'a {
    let mut identifiers = admitted.iter();
    let mut failed = false;
    std::iter::from_fn(move || {
        if failed {
            return None;
        }
        if let Err(error) = budget.charge_work(0, "STEP schema identifier diagnostic traversal") {
            failed = true;
            return Some(Err(error));
        }
        loop {
            if identifiers.len() == 0 {
                return None;
            }
            let identifier = match budget.next_charged(&mut identifiers, "STEP schema identifier diagnostic traversal") {
                Ok(Some(identifier)) => identifier,
                Ok(None) => return None,
                Err(error) => {
                    failed = true;
                    return Some(Err(error));
                }
            };
            let Some((name, component)) = identifier.out_of_range() else {
                continue;
            };
            let diagnostic = budget.format_retained(format_args!(
                    "FILE_SCHEMA identifier {name} has an out-of-range object identifier component {component}; the object identifier is not admitted"
                ), "step_schema_oid_diagnostic_text")
            .map(|message| ParseDiagnostic {
                offset,
                kind: ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange,
                message,
            });
            failed = diagnostic.is_err();
            return Some(diagnostic);
        }
    })
}

enum HeaderDataReferences {
    FilePopulation(BTreeSet<String>),
    Section(String),
}

fn validate_header_sections(
    implementation_level: ImplementationLevel,
    header: &[HeaderRecord],
    schema_identifiers: &[String],
    budget: &DecodeContext<'_>,
) -> Result<Vec<HeaderDataReferences>, ValidationError> {
    let has = |name: &str| -> Result<bool, CodecError> {
        budget.any_by(
            header,
            |record| Ok(record.name == name),
            "STEP header section name search",
        )
    };
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("FILE_POPULATION")? {
        return invalid("2;1 forbids FILE_POPULATION in HEADER");
    }
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("SECTION_LANGUAGE")? {
        return invalid("2;1 forbids SECTION_LANGUAGE in HEADER");
    }
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("SECTION_CONTEXT")? {
        return invalid("2;1 forbids SECTION_CONTEXT in HEADER");
    }
    match implementation_level {
        ImplementationLevel::LegacyEdition2 if has("SCHEMA_POPULATION")? => {
            return invalid("3;1 forbids SCHEMA_POPULATION in HEADER");
        }
        ImplementationLevel::Edition3Class1 if has("SCHEMA_POPULATION")? => {
            return invalid("4;1 forbids SCHEMA_POPULATION in HEADER");
        }
        _ => {}
    }

    let mut references = Vec::new();
    let mut user_defined = false;
    let mut schema_population_seen = false;
    let mut uniqueness_storage =
        budget.reserve_scoped(0, "STEP header section uniqueness storage")?;
    let mut language_sections = BTreeSet::new();
    let mut context_sections = BTreeSet::new();
    let mut optional = header.get(3..).unwrap_or_default().iter();
    budget.charge_work(0, "STEP validate header sections traversal")?;
    for _ in 0..optional.len() {
        let record = budget.next_charged(&mut optional, "STEP validate header sections traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        if record.name.starts_with('!') {
            user_defined = true;
            continue;
        }
        if user_defined {
            return invalid("built-in HEADER entities must precede user-defined entities");
        }
        match record.name.as_str() {
            "SCHEMA_POPULATION" => {
                if schema_population_seen {
                    return invalid("HEADER contains duplicate SCHEMA_POPULATION");
                }
                schema_population_seen = true;
                if !valid_schema_population(&record.parameters, implementation_level, budget)? {
                    return invalid("SCHEMA_POPULATION has invalid parameters");
                }
            }
            "FILE_POPULATION" => {
                let sections = admit_file_population(
                    &record.parameters,
                    schema_identifiers,
                    implementation_level,
                    budget,
                )
                .map_err(|error| error.with_message("FILE_POPULATION has invalid parameters"))?;
                budget
                    .push_vec(
                        &mut references,
                        HeaderDataReferences::FilePopulation(sections),
                        "step_header_data_references",
                    )
                    .map_err(ValidationError::Resource)?;
            }
            "SECTION_LANGUAGE" => {
                let section =
                    valid_section_language(&record.parameters, implementation_level, budget)
                        .map_err(|error| {
                            error.with_message("SECTION_LANGUAGE has invalid parameters")
                        })?;
                let inserted = uniqueness_storage.with_storage(|| {
                    let copied = section
                        .as_deref()
                        .map(|value| {
                            budget.copy_retained_text(value, "step_section_language_name_copy")
                        })
                        .transpose()?;
                    budget.insert_btree_set(
                        &mut language_sections,
                        copied,
                        "step_section_language_names",
                    )
                })?;
                if !inserted {
                    return invalid("HEADER contains duplicate SECTION_LANGUAGE section");
                }
                if let Some(section) = section {
                    budget
                        .push_vec(
                            &mut references,
                            HeaderDataReferences::Section(section),
                            "step_header_data_references",
                        )
                        .map_err(ValidationError::Resource)?;
                }
            }
            "SECTION_CONTEXT" => {
                let section =
                    valid_section_context(&record.parameters, implementation_level, budget)
                        .map_err(|error| {
                            error.with_message("SECTION_CONTEXT has invalid parameters")
                        })?;
                let inserted = uniqueness_storage.with_storage(|| {
                    let copied = section
                        .as_deref()
                        .map(|value| {
                            budget.copy_retained_text(value, "step_section_context_name_copy")
                        })
                        .transpose()?;
                    budget.insert_btree_set(
                        &mut context_sections,
                        copied,
                        "step_section_context_names",
                    )
                })?;
                if !inserted {
                    return invalid("HEADER contains duplicate SECTION_CONTEXT section");
                }
                if let Some(section) = section {
                    budget
                        .push_vec(
                            &mut references,
                            HeaderDataReferences::Section(section),
                            "step_header_data_references",
                        )
                        .map_err(ValidationError::Resource)?;
                }
            }
            _ => return invalid("HEADER contains an unsupported entity"),
        }
    }
    Ok(references)
}

fn valid_schema_population(
    parameters: &[Value],
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let [Value::List(identifications)] = parameters else {
        return Ok(false);
    };
    if identifications.is_empty() {
        return Ok(false);
    }
    budget.all_by(
        identifications.as_slice(),
        |identification| {
            let Value::List(values) = identification else {
                return Ok(false);
            };
            let [address, time_stamp, digest] = values.as_slice() else {
                return Ok(false);
            };
            Ok(is_decodable_string(address, implementation_level, budget)?
                && valid_optional_timestamp(time_stamp, implementation_level, budget)?
                && valid_optional_base64(digest, implementation_level, budget)?)
        },
        "STEP schema population traversal",
    )
}

fn admit_file_population(
    parameters: &[Value],
    schema_identifiers: &[String],
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<BTreeSet<String>, ValidationError> {
    let [Value::String(schema), Value::String(_), governed_sections] = parameters else {
        return invalid("FILE_POPULATION has invalid parameters");
    };
    let (decoded_schema, _schema_storage) = budget
        .with_scoped_storage("STEP population schema validation storage", || {
            decoded_bytes(schema, implementation_level, budget)
        })?;
    let Some(schema) = decoded_schema else {
        return invalid("FILE_POPULATION has invalid parameters");
    };
    if !valid_schema_identifier(budget, &schema)?
        || !is_decodable_string(&parameters[1], implementation_level, budget)?
        || !schema_identifier_matches(schema_identifiers, &schema, budget)?
    {
        return invalid("FILE_POPULATION has invalid parameters");
    }
    match governed_sections {
        Value::Omitted => Ok(BTreeSet::new()),
        Value::List(sections) if !sections.is_empty() => {
            let mut names = BTreeSet::new();
            let mut sections = sections.iter();
            budget.charge_work(0, "STEP file population section traversal")?;
            for _ in 0..sections.len() {
                let section = budget.next_charged(&mut sections, "STEP file population section traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                let Some(section) = decoded_string(section, implementation_level, budget)? else {
                    return invalid("FILE_POPULATION has invalid parameters");
                };
                if !budget.insert_btree_set(&mut names, section, "step_file_population_sections")? {
                    return invalid("FILE_POPULATION has invalid parameters");
                }
            }
            Ok(names)
        }
        _ => invalid("FILE_POPULATION has invalid parameters"),
    }
}

fn valid_section_language(
    parameters: &[Value],
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<Option<String>, ValidationError> {
    let [section, language] = parameters else {
        return invalid("SECTION_LANGUAGE has invalid parameters");
    };
    let (decoded_language, _language_storage) = budget
        .with_scoped_storage("STEP section language validation storage", || {
            decoded_string(language, implementation_level, budget)
        })?;
    let Some(language) = decoded_language else {
        return invalid("SECTION_LANGUAGE has invalid parameters");
    };
    if language.len() != 3 || !language.as_bytes().iter().all(u8::is_ascii_alphabetic) {
        return invalid("SECTION_LANGUAGE has invalid parameters");
    }
    valid_optional_section_name(section, implementation_level, budget)
}

fn valid_section_context(
    parameters: &[Value],
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<Option<String>, ValidationError> {
    let [section, Value::List(contexts)] = parameters else {
        return invalid("SECTION_CONTEXT has invalid parameters");
    };
    if contexts.is_empty() {
        return invalid("SECTION_CONTEXT has invalid parameters");
    }
    if !budget.all_by(
        contexts.as_slice(),
        |context| is_decodable_string(context, implementation_level, budget),
        "STEP section context traversal",
    )? {
        return invalid("SECTION_CONTEXT has invalid parameters");
    }
    valid_optional_section_name(section, implementation_level, budget)
}

fn valid_optional_section_name(
    value: &Value,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<Option<String>, ValidationError> {
    match value {
        Value::Omitted => Ok(None),
        value => decoded_string(value, implementation_level, budget)?
            .map(Some)
            .ok_or(ValidationError::Invalid("invalid section name")),
    }
}

fn is_decodable_string(
    value: &Value,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(false);
    };
    match crate::strings::decoded_char_count(budget, bytes, implementation_level) {
        Ok(_) => Ok(true),
        Err(crate::strings::StringDecodeFailure::Invalid(_)) => Ok(false),
        Err(crate::strings::StringDecodeFailure::Resource(error)) => Err(error),
    }
}

fn is_decodable_string_list(
    value: Option<&Value>,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(Value::List(values)) = value else {
        return Ok(false);
    };
    if values.is_empty() {
        return Ok(false);
    }
    budget.all_by(
        values.as_slice(),
        |value| is_decodable_string(value, implementation_level, budget),
        "STEP header string traversal",
    )
}

fn is_decodable_string_or_omitted(
    value: &Value,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if matches!(value, Value::Omitted) {
        return Ok(true);
    }
    is_decodable_string(value, implementation_level, budget)
}

fn string_within_limit(
    value: &Value,
    implementation_level: ImplementationLevel,
    limit: usize,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(false);
    };
    match crate::strings::decoded_char_count(budget, bytes, implementation_level) {
        Ok(count) => Ok(count <= limit),
        Err(crate::strings::StringDecodeFailure::Invalid(_)) => Ok(false),
        Err(crate::strings::StringDecodeFailure::Resource(error)) => Err(error),
    }
}

fn string_list_within_limit(
    value: Option<&Value>,
    implementation_level: ImplementationLevel,
    limit: usize,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(Value::List(values)) = value else {
        return Ok(false);
    };
    if values.is_empty() {
        return Ok(false);
    }
    budget.all_by(
        values.as_slice(),
        |value| string_within_limit(value, implementation_level, limit, budget),
        "STEP header string traversal",
    )
}

fn string_or_omitted_within_limit(
    value: &Value,
    implementation_level: ImplementationLevel,
    limit: usize,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if matches!(value, Value::Omitted) {
        return Ok(true);
    }
    string_within_limit(value, implementation_level, limit, budget)
}

fn valid_optional_timestamp(
    value: &Value,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    match value {
        Value::Omitted => Ok(true),
        Value::String(_) => match budget
            .with_scoped_storage("STEP optional header string validation storage", || {
                decoded_string(value, implementation_level, budget)
            })? {
            (Some(value), _storage) => valid_timestamp_text(budget, &value),
            (None, _) => Ok(false),
        },
        _ => Ok(false),
    }
}

fn valid_timestamp_text(budget: &DecodeContext<'_>, value: &str) -> Result<bool, CodecError> {
    let bytes = value.as_bytes();
    if bytes.len() < 19
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !all_ascii_digits(&bytes[0..4])
        || !all_ascii_digits(&bytes[5..7])
        || !all_ascii_digits(&bytes[8..10])
        || !all_ascii_digits(&bytes[11..13])
        || !all_ascii_digits(&bytes[14..16])
        || !all_ascii_digits(&bytes[17..19])
    {
        return Ok(false);
    }
    let year = parse_ascii_digits(&bytes[0..4]);
    let month = parse_ascii_digits(&bytes[5..7]);
    let day = parse_ascii_digits(&bytes[8..10]);
    let hour = parse_ascii_digits(&bytes[11..13]);
    let minute = parse_ascii_digits(&bytes[14..16]);
    let second = parse_ascii_digits(&bytes[17..19]);
    let Some(days) = days_in_month(year, month) else {
        return Ok(false);
    };
    if day == 0
        || day > days
        || minute > 59
        || second > 60
        || hour > 24
        || (hour == 24 && (minute != 0 || second != 0))
    {
        return Ok(false);
    }

    let mut at = 19;
    if matches!(bytes.get(at), Some(b'.' | b',')) {
        at += 1;
        let fraction_start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            budget.charge_work(1, "STEP timestamp fraction traversal")?;
            at += 1;
        }
        if at == fraction_start {
            return Ok(false);
        }
    }
    Ok(match bytes.get(at).copied() {
        None => true,
        Some(b'Z') => at + 1 == bytes.len(),
        Some(b'+' | b'-') => {
            at += 1;
            at + 5 == bytes.len()
                && all_ascii_digits(&bytes[at..at + 2])
                && bytes[at + 2] == b':'
                && all_ascii_digits(&bytes[at + 3..at + 5])
                && parse_ascii_digits(&bytes[at..at + 2]) <= 23
                && parse_ascii_digits(&bytes[at + 3..at + 5]) <= 59
        }
        Some(_) => false,
    })
}

fn days_in_month(year: usize, month: usize) -> Option<usize> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => return None,
    };
    Some(days)
}

// Timestamp fields contain two or four decimal bytes.
fn all_ascii_digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

fn parse_ascii_digits(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + usize::from(byte - b'0'))
}

fn valid_optional_base64(
    value: &Value,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    match value {
        Value::Omitted => Ok(true),
        Value::String(_) => match budget
            .with_scoped_storage("STEP optional header string validation storage", || {
                decoded_string(value, implementation_level, budget)
            })? {
            (Some(value), _storage) => valid_base64_text(budget, value.as_bytes()),
            (None, _) => Ok(false),
        },
        _ => Ok(false),
    }
}

fn valid_base64_text(budget: &DecodeContext<'_>, bytes: &[u8]) -> Result<bool, CodecError> {
    if bytes.is_empty() {
        return Ok(false);
    }
    let mut quantum_len = 0;
    let mut padding = 0;
    let mut finished = false;
    let mut at = 0;
    while at < bytes.len() {
        budget.charge_work(1, "STEP base64 validation traversal")?;
        let byte = bytes[at];
        at += 1;
        let is_alphabet = byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/');
        if finished || (padding != 0 && is_alphabet) {
            return Ok(false);
        }
        if is_alphabet {
            quantum_len += 1;
        } else if byte == b'=' {
            if quantum_len < 2 || padding == 2 {
                return Ok(false);
            }
            padding += 1;
            quantum_len += 1;
        } else {
            return Ok(false);
        }
        if quantum_len == 4 {
            finished = padding != 0;
            quantum_len = 0;
            padding = 0;
        }
    }
    Ok(quantum_len == 0)
}

fn decoded_string(
    value: &Value,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(None);
    };
    decoded_bytes(bytes, implementation_level, budget)
}

fn decoded_bytes(
    bytes: &[u8],
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let result = crate::strings::decode_with_context(bytes, implementation_level, budget);
    match result {
        Ok(value) => Ok(Some(value)),
        Err(crate::strings::StringDecodeFailure::Invalid(_)) => Ok(None),
        Err(crate::strings::StringDecodeFailure::Resource(error)) => Err(error),
    }
}

fn schema_identifier_matches(
    schema_identifiers: &[String],
    schema_name: &str,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let trimmed = budget.trim_text(schema_name, "STEP schema name match trim")?;
    let (mut schema_name, _storage) = budget
        .with_scoped_storage("step_schema_name_matching", || {
            budget.copy_retained_text(trimmed, "step_schema_name_matching")
        })?;
    budget.make_ascii_uppercase(&mut schema_name, "STEP schema name uppercase")?;
    budget.any_by(
        schema_identifiers,
        |identifier| {
            let identifier =
                budget.trim_text(identifier.as_str(), "STEP matching schema identifier trim")?;
            if budget.equal(
                identifier,
                schema_name.as_str(),
                "STEP full schema identifier equality",
            )? {
                return Ok(true);
            }
            if let Some((name, _)) = split_schema_identifier(budget, identifier)? {
                return budget.equal(
                    name,
                    schema_name.as_str(),
                    "STEP schema identifier prefix equality",
                );
            }
            Ok(false)
        },
        "STEP schema identifier matches traversal",
    )
}

fn validate_header_data_references(
    budget: &DecodeContext<'_>,
    references: &[HeaderDataReferences],
    data_section_names: &BTreeSet<String>,
) -> Result<(), ValidationError> {
    let mut references = references.iter();
    budget.charge_work(0, "STEP header DATA reference traversal")?;
    for _ in 0..references.len() {
        let reference = budget.next_charged(&mut references, "STEP header DATA reference traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        match reference {
            HeaderDataReferences::FilePopulation(sections) => {
                let mut sections = sections.iter();
                budget.charge_work(0, "STEP FILE_POPULATION section traversal")?;
                for _ in 0..sections.len() {
                    let section = budget.next_charged(&mut sections, "STEP FILE_POPULATION section traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    if !budget.contains_btree_set(
                        data_section_names,
                        section,
                        "STEP DATA section name lookup",
                    )? {
                        return invalid("FILE_POPULATION names an unknown DATA section");
                    }
                }
            }
            HeaderDataReferences::Section(section) => {
                if !budget.contains_btree_set(
                    data_section_names,
                    section,
                    "STEP DATA section name lookup",
                )? {
                    return invalid("header section reference names an unknown DATA section");
                }
            }
        }
    }
    Ok(())
}

fn valid_data_parameters(
    parameters: &[Value],
    schema_identifiers: &[String],
    implementation_level: ImplementationLevel,
    section_names: &mut BTreeSet<String>,
    budget: &DecodeContext<'_>,
) -> Result<(), ValidationError> {
    let [Value::String(section_name), Value::List(schema)] = parameters else {
        return invalid("DATA section parameters must contain a name and one schema");
    };
    let [Value::String(schema_name)] = schema.as_slice() else {
        return invalid("DATA section parameters must contain a name and one schema");
    };
    let Some(section_name) = decoded_bytes(section_name, implementation_level, budget)? else {
        return invalid("DATA section parameters contain an invalid string");
    };
    if !budget.insert_btree_set(section_names, section_name, "step_data_section_names")? {
        return invalid("DATA section names must be unique");
    }
    let (decoded_schema, _schema_storage) = budget
        .with_scoped_storage("STEP data schema validation storage", || {
            decoded_bytes(schema_name, implementation_level, budget)
        })?;
    let Some(schema_name) = decoded_schema else {
        return invalid("DATA section parameters contain an invalid string");
    };
    if !valid_schema_identifier(budget, &schema_name)?
        || !schema_identifier_matches(schema_identifiers, &schema_name, budget)?
    {
        return invalid("DATA section schema is not listed in FILE_SCHEMA");
    }
    Ok(())
}

/// The admitted `FILE_SCHEMA` identifiers, for schema-name matching.
fn schema_names_for_matching(
    admitted: &[AdmittedSchemaIdentifier],
    budget: &DecodeContext<'_>,
) -> Result<Vec<String>, ParseError> {
    let mut names = Vec::new();
    let mut visited_items = (admitted).iter();
    budget.charge_work(0, "STEP schema names for matching traversal")?;
    for _ in 0..visited_items.len() {
        let identifier = budget.next_charged(&mut visited_items, "STEP schema names for matching traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        let source = identifier.text();
        let mut name = budget.copy_retained_text(source, "step_schema_matching_name")?;
        budget.make_ascii_uppercase(&mut name, "STEP schema matching name uppercase")?;
        budget.push_vec(&mut names, name, "step_schema_matching_names")?;
    }
    Ok(names)
}

fn is_string_list(budget: &DecodeContext<'_>, value: Option<&Value>) -> Result<bool, CodecError> {
    match value {
        Some(Value::List(values)) if !values.is_empty() => budget.all_by(
            values.as_slice(),
            |value| Ok(matches!(value, Value::String(_))),
            "STEP string list traversal",
        ),
        _ => Ok(false),
    }
}

fn is_string_or_omitted(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::String(_) | Value::Omitted))
}

fn valid_anchor_name(budget: &DecodeContext<'_>, name: &str) -> Result<bool, CodecError> {
    Ok(!name.is_empty()
        && budget.any_by(
            name.as_bytes(),
            |byte| Ok(!byte.is_ascii_digit()),
            "STEP anchor name traversal",
        )?)
}

fn is_anchor_item(budget: &DecodeContext<'_>, value: &Value) -> Result<bool, CodecError> {
    let _depth = budget.enter_nested("STEP anchor item classification")?;
    match value {
        Value::Reference(_)
        | Value::ExternalReference(_)
        | Value::ConstantEntity(_)
        | Value::ExpressValueConstant(_)
        | Value::Integer(_)
        | Value::Real(_)
        | Value::Enumeration(_)
        | Value::String(_)
        | Value::Binary(_)
        | Value::Resource(_)
        | Value::Omitted => Ok(true),
        Value::List(values) => budget.all_by(
            values.as_slice(),
            |value| is_anchor_item(budget, value),
            "STEP anchor item traversal",
        ),
        Value::Derived | Value::Typed(_, _) => Ok(false),
    }
}

#[derive(Debug)]
enum ResolveError {
    Syntax(String),
    Resource(CodecError),
}
impl From<cadmpeg_core::decode::ResourceLimit> for ResolveError {
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(error.into())
    }
}

impl ResolveError {
    fn into_parse_error(self, offset: usize) -> ParseError {
        match self {
            Self::Syntax(message) => ParseError::Syntax { offset, message },
            Self::Resource(error) => ParseError::Resource(error),
        }
    }
}

impl From<CodecError> for ResolveError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

fn collection_cap(budget: &DecodeContext<'_>, format_cap: usize) -> usize {
    usize::try_from(budget.policy().limits.max_collection_items)
        .map_or(format_cap, |policy| policy.min(format_cap))
}

fn recursion_cap(budget: &DecodeContext<'_>, format_cap: usize) -> usize {
    usize::try_from(budget.policy().limits.max_recursion_depth)
        .map_or(format_cap, |policy| policy.min(format_cap))
}

struct AnchorResolver<'a, 'ctx, 'arena> {
    anchors: &'a BTreeMap<String, Value>,
    memo: BTreeMap<&'a str, (Value, usize)>,
    remaining_nodes: usize,
    budget: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx, 'arena> AnchorResolver<'a, 'ctx, 'arena> {
    const MAX_EXPANDED_NODES: usize = 1_000_000;
    const MAX_REFERENCE_DEPTH: usize = 256;

    fn new(
        anchors: &'a BTreeMap<String, Value>,
        budget: &'ctx DecodeContext<'arena>,
    ) -> Result<Self, ResolveError> {
        Ok(Self {
            storage: budget.reserve_scoped(0, "step_anchor_resolver_storage")?,
            anchors,
            memo: BTreeMap::new(),
            remaining_nodes: collection_cap(budget, Self::MAX_EXPANDED_NODES),
            budget,
        })
    }

    fn resolve_root(&mut self, value: &Value) -> Result<Value, ResolveError> {
        let mut stack_storage = self
            .budget
            .reserve_scoped(0, "step_anchor_reference_stack_storage")?;
        let mut stack = Vec::new();
        let (value, _, expanded_nodes) = self.resolve(
            value,
            &mut stack,
            &mut stack_storage,
            self.remaining_nodes,
            0,
        )?;
        self.remaining_nodes = self
            .remaining_nodes
            .checked_sub(expanded_nodes)
            .ok_or_else(|| self.node_limit_error())?;
        Ok(value)
    }

    fn node_limit_error(&self) -> ResolveError {
        self.budget
            .refuse_codec_limit(
                "step_anchor_output_node_limit",
                u64_from_index(collection_cap(self.budget, Self::MAX_EXPANDED_NODES)),
                u64_from_index(collection_cap(self.budget, Self::MAX_EXPANDED_NODES) + 1),
            )
            .into()
    }

    fn charge_nodes(&self, count: usize) -> Result<(), ResolveError> {
        let count = u64_from_index(count);
        self.budget
            .charge_collection_items(count, "step_anchor_materialization")
            .map_err(ResolveError::Resource)?;
        Ok(())
    }

    fn resolve(
        &mut self,
        value: &Value,
        stack: &mut Vec<&'a str>,
        stack_storage: &mut ScopedReservation<'ctx>,
        budget: usize,
        depth: usize,
    ) -> Result<(Value, usize, usize), ResolveError> {
        let _nested = self
            .budget
            .enter_nested("step_anchor_reference")
            .map_err(ResolveError::Resource)?;
        self.budget.charge_work(1, "step_anchor_materialization")?;
        if depth >= recursion_cap(self.budget, Self::MAX_REFERENCE_DEPTH) {
            return Err(self
                .budget
                .refuse_codec_limit(
                    "step_anchor_depth_limit",
                    u64_from_index(recursion_cap(self.budget, Self::MAX_REFERENCE_DEPTH)),
                    u64_from_index(depth + 1),
                )
                .into());
        }
        if let Value::Resource(name) = value {
            if let Some((name, source)) = self.budget.get_key_value_btree_map(
                self.anchors,
                name.as_str(),
                "STEP anchor binding lookup",
            )? {
                let name = name.as_str();
                if let Some((value, nodes)) =
                    self.budget
                        .get_btree_map(&self.memo, name, "STEP anchor memo lookup")?
                {
                    if *nodes > budget {
                        return Err(self.node_limit_error());
                    }
                    self.charge_nodes(*nodes)?;
                    return Ok((
                        try_clone_value(value, self.budget, "step_anchor_memo_value_copy")
                            .map_err(ResolveError::Resource)?,
                        *nodes,
                        *nodes,
                    ));
                }
                if self.budget.any_by(
                    stack.as_slice(),
                    |previous| {
                        self.budget
                            .equal(*previous, name, "STEP anchor cycle name equality")
                    },
                    "STEP anchor cycle search",
                )? {
                    let message = self
                        .budget
                        .format_retained(
                            format_args!("cyclic anchor binding <{name}>"),
                            "step_cyclic_anchor_error_text",
                        )
                        .map_err(ResolveError::Resource)?;
                    return Err(ResolveError::Syntax(message));
                }
                value_node_count(source, Self::MAX_EXPANDED_NODES, self.budget)?;

                stack_storage
                    .with_storage(|| {
                        self.budget
                            .charge_collection_items(1, "step_anchor_reference_stack")?;
                        self.budget.reserve_capacity(
                            stack,
                            1,
                            "step_anchor_reference_stack_storage",
                        )
                    })
                    .map_err(ResolveError::Resource)?;
                stack.push(name);
                let resolved = self.resolve(source, stack, stack_storage, budget, depth + 1);
                stack.pop();
                let (value, nodes, _) = resolved?;
                if nodes > budget {
                    return Err(self.node_limit_error());
                }
                self.charge_nodes(nodes)?;
                self.storage
                    .with_storage(|| {
                        let entry = self.budget.entry_btree_map(
                            &mut self.memo,
                            name,
                            "step_anchor_memo_entry",
                        )?;
                        let copied =
                            try_clone_value(&value, self.budget, "step_anchor_memo_value_copy")?;
                        entry.or_insert((copied, nodes));
                        Ok::<(), CodecError>(())
                    })
                    .map_err(ResolveError::Resource)?;
                return Ok((value, nodes, nodes));
            }
        }
        match value {
            Value::List(values) => {
                self.charge_nodes(1)?;
                let mut nodes = 1usize;
                let mut expanded_nodes = 0usize;
                let mut resolved = self
                    .budget
                    .collection_vec(values.len(), "step_anchor_list_items")
                    .map_err(ResolveError::Resource)?;
                let mut values = values.iter();
                self.budget.charge_work(0, "STEP resolve value traversal")?;
                for _ in 0..values.len() {
                    let value = self.budget.next_charged(&mut values, "STEP resolve value traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    let remaining = budget
                        .checked_sub(expanded_nodes)
                        .ok_or_else(|| self.node_limit_error())?;
                    let (value, child_nodes, child_expanded_nodes) =
                        self.resolve(value, stack, stack_storage, remaining, depth + 1)?;
                    nodes = nodes
                        .checked_add(child_nodes)
                        .ok_or_else(|| self.node_limit_error())?;
                    expanded_nodes = expanded_nodes
                        .checked_add(child_expanded_nodes)
                        .ok_or_else(|| self.node_limit_error())?;
                    resolved.push(value);
                }
                Ok((Value::List(resolved), nodes, expanded_nodes))
            }
            Value::Typed(name, value) => {
                self.charge_nodes(1)?;
                let (value, nodes, expanded_nodes) =
                    self.resolve(value, stack, stack_storage, budget, depth + 1)?;
                self.budget
                    .charge_retained(
                        u64_from_index(size_of::<Value>()),
                        "step_anchor_materialization_storage",
                    )
                    .map_err(ResolveError::Resource)?;
                let value = Value::Typed(
                    self.budget
                        .copy_retained_text(name, "step_anchor_typed_name_copy")
                        .map_err(ResolveError::Resource)?,
                    Box::new(value),
                );
                Ok((
                    value,
                    nodes
                        .checked_add(1)
                        .ok_or_else(|| self.node_limit_error())?,
                    expanded_nodes,
                ))
            }
            value => {
                self.charge_nodes(1)?;
                Ok((
                    try_clone_value(value, self.budget, "step_anchor_leaf_copy")
                        .map_err(ResolveError::Resource)?,
                    1,
                    0,
                ))
            }
        }
    }
}

struct ReferenceResolver<'a, 'ctx, 'arena> {
    bindings: BTreeMap<ReferenceName, &'a str>,
    anchors: &'a BTreeMap<String, Value>,
    stack: Vec<ReferenceName>,
    remaining_nodes: usize,
    budget: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx, 'arena> ReferenceResolver<'a, 'ctx, 'arena> {
    const MAX_MATERIALIZED_NODES: usize = 1_000_000;
    const MAX_REFERENCE_DEPTH: usize = 256;

    fn new(
        references: &'a [ReferenceEntry],
        anchors: &'a BTreeMap<String, Value>,
        budget: &'ctx DecodeContext<'arena>,
    ) -> Result<Self, ResolveError> {
        let mut storage = budget.reserve_scoped(0, "step_reference_binding_storage")?;
        let mut bindings = BTreeMap::new();
        let mut visited_items = (references).iter();
        budget.charge_work(0, "STEP new traversal")?;
        for _ in 0..visited_items.len() {
            let reference = budget.next_charged(&mut visited_items, "STEP new traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            storage.with_storage(|| {
                budget.insert_btree_map(
                    &mut bindings,
                    reference.name,
                    reference.uri.as_str(),
                    "step_reference_binding_items",
                )
            })?;
        }
        Ok(Self {
            storage,
            bindings,
            anchors,
            stack: Vec::new(),
            remaining_nodes: collection_cap(budget, Self::MAX_MATERIALIZED_NODES),
            budget,
        })
    }

    fn admit_copy(&self, nodes: u64) -> Result<(), ResolveError> {
        self.budget
            .charge_collection_items(nodes, "step_reference_materialization")
            .map_err(ResolveError::Resource)?;
        Ok(())
    }

    fn clone_leaf(&mut self, value: &Value) -> Result<Value, ResolveError> {
        self.consume_materialized_node()?;
        self.admit_copy(1)?;
        try_clone_value(value, self.budget, "step_reference_leaf_copy")
            .map_err(ResolveError::Resource)
    }

    fn resolve_value(&mut self, value: &Value, depth: usize) -> Result<Value, ResolveError> {
        let _nested = self
            .budget
            .enter_nested("step_reference_expansion")
            .map_err(ResolveError::Resource)?;
        self.budget
            .charge_work(1, "step_reference_materialization")?;
        if depth >= recursion_cap(self.budget, Self::MAX_REFERENCE_DEPTH) {
            return Err(self
                .budget
                .refuse_codec_limit(
                    "step_reference_depth_limit",
                    u64_from_index(recursion_cap(self.budget, Self::MAX_REFERENCE_DEPTH)),
                    u64_from_index(depth + 1),
                )
                .into());
        }
        match value {
            Value::Reference(id) => {
                self.resolve_occurrence(ReferenceName::Entity(*id), value, depth)
            }
            Value::ExternalReference(id) => {
                self.resolve_occurrence(ReferenceName::Value(*id), value, depth)
            }
            Value::List(values) => {
                self.consume_materialized_node()?;
                if !self.stack.is_empty() && values.len() > self.remaining_nodes {
                    return Err(self
                        .budget
                        .refuse_codec_limit(
                            "step_reference_output_node_limit",
                            u64_from_index(self.remaining_nodes),
                            u64_from_index(values.len()),
                        )
                        .into());
                }
                self.admit_copy(1)?;
                let mut resolved = self
                    .budget
                    .collection_vec(values.len(), "step_reference_list_items")
                    .map_err(ResolveError::Resource)?;
                let mut visited_items = (values.as_slice()).iter();
                self.budget.charge_work(0, "STEP resolve value value traversal")?;
                for _ in 0..visited_items.len() {
                    let value = self.budget.next_charged(&mut visited_items, "STEP resolve value value traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    resolved.push(self.resolve_value(value, depth + 1)?);
                }
                Ok(Value::List(resolved))
            }
            Value::Typed(name, value) => {
                self.consume_materialized_node()?;
                let resolved = self.resolve_value(value, depth + 1)?;
                self.admit_copy(1)?;
                self.budget.charge_retained(
                    u64_from_index(size_of::<Value>()),
                    "step_reference_materialization_storage",
                )?;
                Ok(Value::Typed(
                    self.budget
                        .copy_retained_text(name, "step_reference_typed_name_copy")
                        .map_err(ResolveError::Resource)?,
                    Box::new(resolved),
                ))
            }
            _ => self.clone_leaf(value),
        }
    }

    fn resolve_occurrence(
        &mut self,
        key: ReferenceName,
        original: &Value,
        depth: usize,
    ) -> Result<Value, ResolveError> {
        let Some(uri) = self
            .budget
            .get_btree_map(&self.bindings, &key, "STEP reference binding lookup")?
            .copied()
        else {
            return self.clone_leaf(original);
        };
        let Some(separator) = self
            .budget
            .position_by(uri.as_bytes(), |byte| Ok(*byte == b'#'), "STEP reference URI fragment split")
            .map_err(ResolveError::Resource)?
        else {
            return self.clone_leaf(&Value::Omitted);
        };
        let (path, fragment) = (&uri[..separator], &uri[separator + 1..]);
        if !path.is_empty() {
            return self.clone_leaf(original);
        }
        if self.budget.any_by(
            self.stack.as_slice(),
            |previous| Ok(*previous == key),
            "STEP reference cycle search",
        )? {
            return self.clone_leaf(&Value::Omitted);
        }
        let Some(anchor) =
            self.budget
                .get_btree_map(self.anchors, fragment, "STEP reference anchor lookup")?
        else {
            return if is_uuid_fragment(fragment) {
                self.clone_leaf(original)
            } else {
                self.clone_leaf(&Value::Omitted)
            };
        };

        self.storage
            .with_storage(|| {
                self.budget
                    .charge_collection_items(1, "step_reference_stack")?;
                self.budget
                    .reserve_capacity(&mut self.stack, 1, "step_reference_stack_storage")
            })
            .map_err(ResolveError::Resource)?;
        self.stack.push(key);
        let resolved = self.resolve_value(anchor, depth + 1);
        self.stack.pop();
        let resolved = resolved?;
        if let Value::Resource(uri) = &resolved {
            return if self
                .budget
                .any_by(uri.as_bytes(), |byte| Ok(*byte == b'#'), "STEP resolved URI fragment containment")
                .map_err(ResolveError::Resource)?
            {
                self.clone_leaf(original)
            } else {
                self.clone_leaf(&Value::Omitted)
            };
        }
        if !reference_target_matches(key, &resolved) {
            return self.clone_leaf(&Value::Omitted);
        }
        Ok(resolved)
    }

    fn consume_materialized_node(&mut self) -> Result<(), ResolveError> {
        if !self.stack.is_empty() {
            self.remaining_nodes = self.remaining_nodes.checked_sub(1).ok_or_else(|| {
                self.budget.refuse_codec_limit(
                    "step_reference_output_node_limit",
                    u64_from_index(collection_cap(self.budget, Self::MAX_MATERIALIZED_NODES)),
                    u64_from_index(collection_cap(self.budget, Self::MAX_MATERIALIZED_NODES) + 1),
                )
            })?;
        }
        Ok(())
    }
}

fn resolve_local_references(
    anchors: &mut [AnchorEntry],
    records: &mut BTreeMap<u64, RawRecord>,
    references: &[ReferenceEntry],
    budget: &DecodeContext<'_>,
) -> Result<(), ResolveError> {
    if references.is_empty() {
        return Ok(());
    }
    let mut snapshot_storage = budget.reserve_scoped(0, "step_reference_anchor_copy_storage")?;
    let mut anchor_bindings = BTreeMap::new();
    let mut visited_items = (anchors[..]).iter();
    budget.charge_work(0, "STEP resolve local references traversal")?;
    for _ in 0..visited_items.len() {
        let anchor = budget.next_charged(&mut visited_items, "STEP resolve local references traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        snapshot_storage.with_storage(|| {
            let name =
                budget.copy_retained_text(&anchor.name, "step_reference_anchor_name_copy")?;
            let value = try_clone_value(&anchor.value, budget, "step_reference_anchor_value_copy")?;
            budget.insert_btree_map(
                &mut anchor_bindings,
                name,
                value,
                "step_reference_anchor_copies",
            )?;
            Ok::<(), CodecError>(())
        })?;
    }
    let mut resolver = ReferenceResolver::new(references, &anchor_bindings, budget)?;
    let mut visited_items = (anchors).iter_mut();
    budget.charge_work(0, "STEP local anchor resolution traversal")?;
    for _ in 0..visited_items.len() {
        let anchor = budget.next_charged(&mut visited_items, "STEP local anchor resolution traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        anchor.value = resolver.resolve_value(&anchor.value, 0)?;
        let mut visited_items = (anchor.tags.as_mut_slice()).iter_mut();
        budget.charge_work(0, "STEP local anchor tag traversal")?;
        for _ in 0..visited_items.len() {
            let tag = budget.next_charged(&mut visited_items, "STEP local anchor tag traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            tag.value = resolver.resolve_value(&tag.value, 0)?;
        }
    }
    let mut visited_items = (records).iter_mut();
    budget.charge_work(0, "STEP local record resolution traversal")?;
    for _ in 0..visited_items.len() {
        let (_, record) = budget.next_charged(&mut visited_items, "STEP local record resolution traversal")?
            .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
        let mut visited_items = (record.partials[..]).iter_mut();
        budget.charge_work(0, "STEP local partial resolution traversal")?;
        for _ in 0..visited_items.len() {
            let partial = budget.next_charged(&mut visited_items, "STEP local partial resolution traversal")?
                .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
            let mut visited_items = (partial.parameters.as_mut_slice()).iter_mut();
            budget.charge_work(0, "STEP local parameter resolution traversal")?;
            for _ in 0..visited_items.len() {
                let value = budget.next_charged(&mut visited_items, "STEP local parameter resolution traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                *value = resolver.resolve_value(value, 0)?;
            }
        }
    }
    Ok(())
}

fn reference_target_matches(name: ReferenceName, value: &Value) -> bool {
    match name {
        ReferenceName::Entity(_) => matches!(value, Value::Reference(_) | Value::ConstantEntity(_)),
        ReferenceName::Value(_) => !matches!(
            value,
            Value::Reference(_) | Value::ConstantEntity(_) | Value::Resource(_)
        ),
    }
}

fn is_uuid_fragment(fragment: &str) -> bool {
    fragment.len() == 36
        && fragment.as_bytes().iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn value_node_count(
    value: &Value,
    limit: usize,
    budget: &DecodeContext<'_>,
) -> Result<usize, ResolveError> {
    fn visit(
        value: &Value,
        remaining: &mut usize,
        budget: &DecodeContext<'_>,
        depth: usize,
    ) -> Result<(), ResolveError> {
        let _depth_guard = budget
            .enter_nested("step_value_node_count")
            .map_err(ResolveError::Resource)?;
        if depth >= 256 {
            return Err(budget
                .refuse_codec_limit(
                    "step_value_node_count_depth_limit",
                    256,
                    u64_from_index(depth + 1),
                )
                .into());
        }
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| budget.refuse_codec_limit("step_value_node_count_node_limit", 0, 1))?;
        budget
            .charge_work(1, "step_value_node_count")
            .map_err(ResolveError::Resource)?;
        match value {
            Value::List(values) => {
                let mut visited_items = (values.as_slice()).iter();
                budget.charge_work(0, "STEP visit value traversal")?;
                for _ in 0..visited_items.len() {
                    let child = budget.next_charged(&mut visited_items, "STEP visit value traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    visit(child, remaining, budget, depth + 1)?;
                }
            }
            Value::Typed(_, child) => visit(child, remaining, budget, depth + 1)?,
            _ => {}
        }
        Ok(())
    }

    let mut remaining = limit;
    visit(value, &mut remaining, budget, 0)?;
    Ok(limit - remaining)
}

fn references(
    value: &Value,
    entity_out: &mut Vec<u64>,
    value_out: &mut Vec<u64>,
    budget: &DecodeContext<'_>,
) -> Result<(), ParseError> {
    let (mut pending, mut pending_storage) =
        budget.temporary_vec(0, "STEP reference worklist storage")?;
    budget.push_scoped_vec(
        &mut pending_storage,
        &mut pending,
        value,
        "step_parse_reference_pending",
    )?;
    while !pending.is_empty() {
        budget.charge_work(1, "STEP value worklist traversal")?;
        let Some(value) = pending.pop() else {
            break;
        };
        match value {
            Value::Reference(id) => {
                budget.push_vec(entity_out, *id, "step_parse_reference_ids")?;
            }
            Value::ExternalReference(id) => {
                budget.push_vec(value_out, *id, "step_parse_value_reference_ids")?;
            }
            Value::List(values) => {
                let mut visited_items = (values[..]).iter().rev();
                budget.charge_work(0, "STEP references traversal")?;
                for _ in 0..visited_items.len() {
                    let child = budget.next_charged(&mut visited_items, "STEP references traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP bounded traversal source ended early"))?;
                    budget.push_scoped_vec(
                        &mut pending_storage,
                        &mut pending,
                        child,
                        "step_parse_reference_pending",
                    )?;
                }
            }
            Value::Typed(_, value) => {
                budget.push_scoped_vec(
                    &mut pending_storage,
                    &mut pending,
                    value,
                    "step_parse_reference_pending",
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn contains_class3_occurrence(
    budget: &DecodeContext<'_>,
    value: &Value,
) -> Result<bool, CodecError> {
    let _depth = budget.enter_nested("STEP class-3 occurrence classification")?;
    match value {
        Value::ExternalReference(_) | Value::ConstantEntity(_) | Value::ExpressValueConstant(_) => {
            Ok(true)
        }
        Value::List(values) => budget.any_by(
            values.as_slice(),
            |value| contains_class3_occurrence(budget, value),
            "STEP class-3 occurrence traversal",
        ),
        Value::Typed(_, value) => {
            budget.charge_work(1, "STEP typed class-3 occurrence traversal")?;
            contains_class3_occurrence(budget, value)
        }
        _ => Ok(false),
    }
}

fn contains_resource_value(budget: &DecodeContext<'_>, value: &Value) -> Result<bool, CodecError> {
    let _depth = budget.enter_nested("STEP resource value classification")?;
    match value {
        Value::Resource(_) => Ok(true),
        Value::List(values) => budget.any_by(
            values.as_slice(),
            |value| contains_resource_value(budget, value),
            "STEP resource value traversal",
        ),
        Value::Typed(_, value) => {
            budget.charge_work(1, "STEP typed resource value traversal")?;
            contains_resource_value(budget, value)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests;
