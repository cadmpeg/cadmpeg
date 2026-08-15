// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.
//!
//! The parser accepts only source deviations whose value remains unambiguous:
//! the deviation must be recoverable without guessing, observed in a real
//! producer, represented by its own diagnostic kind, and rejectable by strict
//! decode policy. Ambiguous records and duplicate names remain parse errors.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::mem::size_of;
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::lex::{BinaryValue, LexError, Lexer, Token, TokenKind};

/// One parsed Part 21 parameter value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[allow(clippy::enum_variant_names)] // STEP names mirror the EXPRESS value kinds.
pub enum Value {
    /// Reference to a DATA entity instance.
    Reference(u64),
    /// Reference to an externally defined value instance.
    ValueReference(u64),
    /// Reference to an EXPRESS entity constant.
    ConstantEntity(String),
    /// Reference to an EXPRESS value constant.
    ConstantValue(String),
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
    /// Standard or user-defined type name and its single wrapped parameter.
    Typed(String, Box<Value>),
}

/// One simple entity leaf within an entity instance.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialRecord {
    /// Uppercase standard or `!`-prefixed user-defined entity name.
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
#[non_exhaustive]
pub struct AnchorEntry {
    /// Local resource name.
    pub name: String,
    /// Value bound to the resource name.
    pub value: Value,
    /// Ordered metadata tags attached to the binding.
    pub tags: Vec<AnchorTag>,
}

/// One edition-3 metadata tag attached to an ANCHOR binding.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AnchorTag {
    /// Tag name, preserving source case.
    pub name: String,
    /// Tag value.
    pub value: Value,
}

/// One edition-3 external REFERENCE binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEntry {
    /// External entity or value occurrence name such as `#123`.
    pub name: String,
    /// External resource URI.
    pub uri: String,
}

/// One Part 21 edition-3 detached CMS signature section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureSection {
    /// Complete `SIGNATURE;...ENDSEC;` byte range.
    pub span: Range<usize>,
    /// Base64 payload byte range between the section delimiters.
    pub payload: Range<usize>,
    /// Exchange byte range authenticated by this signature before alphabet
    /// filtering. The range starts at `ISO-10303-21;` and ends at the `S` in
    /// this section's `SIGNATURE;` token.
    pub signed: Range<usize>,
    /// Decoded CMS `SignedData` payload.
    pub cms: Vec<u8>,
}

impl SignatureSection {
    /// Returns the Part 21 alphabet bytes covered by this signature.
    ///
    /// The source range is retained separately because the signature input is
    /// defined by the alphabet projection, not by transport controls such as
    /// line endings. `None` means that the supplied source does not contain
    /// the recorded range.
    #[allow(dead_code)] // Alphabet projection for signature verification; not on the decode path.
    pub fn signed_alphabet_bytes(&self, input: &[u8]) -> Option<Vec<u8>> {
        Some(
            input
                .get(self.signed.clone())?
                .iter()
                .copied()
                .filter(|byte| !byte.is_ascii_control())
                .collect(),
        )
    }
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
    /// Complete SIGNATURE section byte ranges in source order.
    pub signatures: Vec<Range<usize>>,
    /// Parsed signature payload ranges and detached signed-content ranges in
    /// source order.
    pub signature_sections: Vec<SignatureSection>,
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
    /// Release semantic source structures before retained opaque bytes are copied.
    pub(crate) fn release_source_graph(&mut self) {
        self.header.clear();
        self.anchors.clear();
        self.references.clear();
        self.data.clear();
        self.signatures.clear();
        self.signature_sections.clear();
        self.records.clear();
        self.entity_ids = EntityIndex::default();
    }

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
    /// A simple named carrier omits its inherited `name` value.
    OmittedEntityName,
}

/// One attributable parser diagnostic that does not prevent recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    /// Byte offset of the containing source record.
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
        current: None,
        lexer,
        last_end: 0,
        depth: 0,
        diagnostics: Vec::new(),
        omitted_entity_name_count: 0,
        first_omitted_entity_name_offset: None,
        budget,
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
    omitted_entity_name_count: usize,
    first_omitted_entity_name_offset: Option<usize>,
    budget: Option<&'ctx DecodeContext<'arena>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImplementationLevel {
    LegacyEdition1,
    LegacyEdition2,
    Edition3Class1,
    Edition3Class2,
    Edition3Class3,
}

impl ImplementationLevel {
    fn is_edition3(self) -> bool {
        matches!(
            self,
            Self::Edition3Class1 | Self::Edition3Class2 | Self::Edition3Class3
        )
    }

    fn allows_edition3_sections(self) -> bool {
        matches!(self, Self::Edition3Class2 | Self::Edition3Class3)
    }

    fn allows_class3_occurrences(self) -> bool {
        matches!(self, Self::Edition3Class3)
    }
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
        let exchange_start = self.current_offset();
        self.name("ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
        self.name("HEADER")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut header = Vec::new();
        while !self.peek_name("ENDSEC") {
            let name = self.take_name()?;
            self.charge_string_storage(&name, "step_parse_name_storage")?;
            let parameters = self.parameters()?;
            self.charge_value_vec_storage(&parameters, "step_parse_collection_storage")?;
            self.punct(&TokenKind::Semicolon)?;
            header.push(HeaderRecord { name, parameters });
        }
        self.name("ENDSEC")?;
        self.punct(&TokenKind::Semicolon)?;
        let implementation_level = match validate_header(&header) {
            Ok(level) => level,
            Err(message) => return self.err(message),
        };
        let schema_identifiers = schema_identifiers(&header, implementation_level);
        if let Err(message) =
            validate_header_sections(implementation_level, &header, &schema_identifiers)
        {
            return self.err(message);
        }
        let mut anchors = Vec::new();
        if !implementation_level.allows_edition3_sections()
            && (self.peek_name("ANCHOR") || self.peek_name("REFERENCE"))
        {
            return self.err(match implementation_level {
                ImplementationLevel::LegacyEdition1 => "2;1 forbids ANCHOR and REFERENCE sections",
                ImplementationLevel::LegacyEdition2 => "3;1 forbids ANCHOR and REFERENCE sections",
                ImplementationLevel::Edition3Class1 => "4;1 forbids ANCHOR and REFERENCE sections",
                ImplementationLevel::Edition3Class2 | ImplementationLevel::Edition3Class3 => {
                    unreachable!()
                }
            });
        }
        if self.peek_name("ANCHOR") {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                let TokenKind::Resource(name) = self.next_kind()? else {
                    return self.err("expected anchor name");
                };
                if !valid_anchor_name(&name) {
                    return self.err("anchor name must contain a non-digit character");
                }
                self.charge_string_storage(&name, "step_parse_name_storage")?;
                self.punct(&TokenKind::Equals)?;
                let value = self.value()?;
                if !is_anchor_item(&value) {
                    return self.err("invalid anchor item");
                }
                let mut tags = Vec::new();
                while self.peek(&TokenKind::LBrace) {
                    self.next_kind()?;
                    let TokenKind::TagName(name) = self.next_kind()? else {
                        return self.err("expected anchor tag name");
                    };
                    self.charge_string_storage(&name, "step_parse_name_storage")?;
                    self.punct(&TokenKind::Colon)?;
                    let value = self.value()?;
                    if !is_anchor_item(&value) {
                        return self.err("invalid anchor tag item");
                    }
                    self.punct(&TokenKind::RBrace)?;
                    tags.push(AnchorTag { name, value });
                }
                tags.shrink_to_fit();
                self.charge_vec_storage(&tags, "step_anchor_tag_storage")?;
                self.punct(&TokenKind::Semicolon)?;
                anchors.push(AnchorEntry { name, value, tags });
            }
            self.next_kind()?;
            self.lexer.set_allow_print_controls(true);
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut reference_entries = Vec::new();
        let mut external_reference_ids = BTreeSet::new();
        let mut external_value_reference_ids = BTreeSet::new();
        let mut reference_names = BTreeSet::new();
        if self.peek_name("REFERENCE") {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                let (name, occurrence_id) = match self.next_kind()? {
                    TokenKind::Instance(id) => (format!("#{id}"), Some((b'#', id))),
                    TokenKind::ValueInstance(id) => (format!("@{id}"), Some((b'@', id))),
                    _ => return self.err("expected reference name"),
                };
                self.charge_string_storage(&name, "step_parse_reference_storage")?;
                if !reference_names.insert(name.clone()) {
                    return self.err("duplicate reference name");
                }
                if let Some((prefix, id)) = occurrence_id {
                    let already_used = external_reference_ids.contains(&id)
                        || external_value_reference_ids.contains(&id);
                    if already_used {
                        return self.err("duplicate external occurrence integer");
                    }
                    match prefix {
                        b'#' => {
                            external_reference_ids.insert(id);
                        }
                        b'@' => {
                            external_value_reference_ids.insert(id);
                        }
                        _ => unreachable!("occurrence prefixes are fixed by the parser"),
                    }
                }
                self.punct(&TokenKind::Equals)?;
                let TokenKind::Resource(uri) = self.next_kind()? else {
                    return self.err("expected reference URI");
                };
                self.charge_string_storage(&uri, "step_parse_reference_storage")?;
                self.punct(&TokenKind::Semicolon)?;
                reference_entries.push(ReferenceEntry { name, uri });
            }
            self.next_kind()?;
            self.lexer.set_allow_print_controls(true);
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut data: Vec<DataSection> = Vec::new();
        let mut records = BTreeMap::new();
        let mut data_section_names = BTreeSet::new();
        while self.peek_name("DATA") {
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
                let parameters = self.parameters()?;
                self.charge_value_vec_storage(&parameters, "step_parse_collection_storage")?;
                if let Err(message) = valid_data_parameters(
                    &parameters,
                    &schema_identifiers,
                    implementation_level,
                    &mut data_section_names,
                ) {
                    return self.err(message);
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
                let record = self.record()?;
                let id = record.id;
                self.charge_retained(
                    btree_node_storage::<u64, RawRecord>(),
                    "step_parse_record_table_storage",
                )?;
                if records.insert(id, record).is_some() {
                    return self.err("duplicate instance name");
                }
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
        if !implementation_level.is_edition3() && data.is_empty() {
            return self.err("historical implementation levels require one DATA section");
        }
        if implementation_level.is_edition3()
            && data.len() == 1
            && data[0].parameters.is_empty()
            && schema_identifier_count(&header) != 1
        {
            return self.err("an unnamed DATA section requires one FILE_SCHEMA identifier");
        }
        if let Err(message) = validate_header_data_references(&header, &data, implementation_level)
        {
            return self.err(message);
        }
        self.name("END-ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut signatures = Vec::new();
        let mut signature_sections = Vec::new();
        if !implementation_level.allows_edition3_sections() && self.peek_name("SIGNATURE") {
            return self.err(match implementation_level {
                ImplementationLevel::LegacyEdition1 => "2;1 forbids SIGNATURE sections",
                ImplementationLevel::LegacyEdition2 => "3;1 forbids SIGNATURE sections",
                ImplementationLevel::Edition3Class1 => "4;1 forbids SIGNATURE sections",
                ImplementationLevel::Edition3Class2 | ImplementationLevel::Edition3Class3 => {
                    unreachable!()
                }
            });
        }
        while self.peek_name("SIGNATURE") {
            let start = self.current_offset();
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            let payload_start = self.last_end;
            let payload_end = self.current_offset();
            while !self.peek_name("ENDSEC") {
                if self.current.is_none() {
                    return self.err("unterminated SIGNATURE section");
                }
                self.next_kind()?;
            }
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            let span = start..self.previous_end();
            let payload = payload_start..payload_end;
            let mut cms = decode_signature_payload(self.lexer.input(), &payload)?;
            cms.shrink_to_fit();
            self.charge_vec_storage(&cms, "step_parse_signature_storage")?;
            signature_sections.push(SignatureSection {
                span: span.clone(),
                payload,
                signed: exchange_start..start,
                cms,
            });
            signatures.push(span);
        }
        if self.current.is_some() {
            return self.err("tokens after exchange terminator");
        }
        if records.keys().any(|id| external_reference_ids.contains(id)) {
            return self.err("external reference instance collides with a DATA instance");
        }
        if records
            .keys()
            .any(|id| external_value_reference_ids.contains(id))
        {
            return self.err("external value instance collides with a DATA instance");
        }
        if !anchors.is_empty() {
            for anchor in &anchors {
                self.charge_string_storage(&anchor.name, "step_anchor_binding_storage")?;
                self.charge_retained(
                    btree_node_storage::<String, Value>(),
                    "step_anchor_binding_storage",
                )?;
            }
            let anchor_bindings = anchors
                .iter()
                .map(|anchor| (anchor.name.clone(), anchor.value.clone()))
                .collect::<BTreeMap<_, _>>();
            if anchor_bindings.len() != anchors.len() {
                return self.err("duplicate anchor name");
            }
            let mut resolver = AnchorResolver::new(&anchor_bindings, self.budget);
            for anchor in &mut anchors {
                anchor.value = resolver
                    .resolve_root(&anchor.value)
                    .map_err(|error| error.into_parse_error(0))?;
                for tag in &mut anchor.tags {
                    tag.value = resolver
                        .resolve_root(&tag.value)
                        .map_err(|error| error.into_parse_error(0))?;
                }
            }
            for record in records.values_mut() {
                for partial in &mut record.partials {
                    for value in &mut partial.parameters {
                        *value = resolver
                            .resolve_root(value)
                            .map_err(|error| error.into_parse_error(record.span.start))?;
                    }
                }
            }
        }
        // Validate the source occurrence class before local REFERENCES can
        // replace a forbidden token with an ordinary value.
        let contains_forbidden_class3_occurrence =
            !implementation_level.allows_class3_occurrences()
                && (header
                    .iter()
                    .any(|record| record.parameters.iter().any(contains_class3_occurrence))
                    || anchors.iter().any(|anchor| {
                        contains_class3_occurrence(&anchor.value)
                            || anchor
                                .tags
                                .iter()
                                .any(|tag| contains_class3_occurrence(&tag.value))
                    })
                    || records.values().any(|record| {
                        record.partials.iter().any(|partial| {
                            partial.parameters.iter().any(contains_class3_occurrence)
                        })
                    }));
        resolve_local_references(&mut anchors, &mut records, &reference_entries, self.budget)
            .map_err(|error| error.into_parse_error(0))?;
        for record in records.values_mut() {
            if record.partials.len() == 1 && omitted_entity_name(&record.partials[0]) {
                let previous_capacity = record.partials[0].parameters.capacity();
                record.partials[0]
                    .parameters
                    .insert(0, Value::String(Vec::new()));
                record.partials[0].parameters.shrink_to_fit();
                let added_capacity = record.partials[0]
                    .parameters
                    .capacity()
                    .saturating_sub(previous_capacity);
                self.charge_retained(
                    allocation_bytes(added_capacity, size_of::<Value>()),
                    "step_omitted_name_recovery_storage",
                )?;
                self.omitted_entity_name_count += 1;
                self.first_omitted_entity_name_offset
                    .get_or_insert(record.span.start);
            }
        }
        let mut refs = Vec::new();
        let mut value_refs = Vec::new();
        for anchor in &anchors {
            refs.clear();
            value_refs.clear();
            references(&anchor.value, &mut refs, &mut value_refs);
            if refs
                .iter()
                .any(|id| !records.contains_key(id) && !external_reference_ids.contains(id))
            {
                return self.err("unresolved instance reference in anchor binding");
            }
            if value_refs
                .iter()
                .any(|id| !external_value_reference_ids.contains(id))
            {
                return self.err("unresolved value instance reference in anchor binding");
            }
            for tag in &anchor.tags {
                refs.clear();
                value_refs.clear();
                references(&tag.value, &mut refs, &mut value_refs);
                if refs
                    .iter()
                    .any(|id| !records.contains_key(id) && !external_reference_ids.contains(id))
                {
                    return self.err("unresolved instance reference in anchor tag");
                }
                if value_refs
                    .iter()
                    .any(|id| !external_value_reference_ids.contains(id))
                {
                    return self.err("unresolved value instance reference in anchor tag");
                }
            }
        }
        for record in records.values() {
            refs.clear();
            value_refs.clear();
            for partial in &record.partials {
                for value in &partial.parameters {
                    references(value, &mut refs, &mut value_refs);
                }
            }
            if refs
                .iter()
                .any(|id| !records.contains_key(id) && !external_reference_ids.contains(id))
            {
                return Self::err_at(record.span.start, "unresolved instance reference");
            }
            if value_refs
                .iter()
                .any(|id| !external_value_reference_ids.contains(id))
            {
                return Self::err_at(record.span.start, "unresolved value instance reference");
            }
        }
        if contains_forbidden_class3_occurrence {
            return self.err(match implementation_level {
                ImplementationLevel::LegacyEdition1 | ImplementationLevel::LegacyEdition2 => {
                    "historical implementation levels forbid edition-3 occurrence names"
                }
                ImplementationLevel::Edition3Class1 | ImplementationLevel::Edition3Class2 => {
                    "this implementation level forbids value instances and EXPRESS constants"
                }
                ImplementationLevel::Edition3Class3 => unreachable!(),
            });
        }
        let has_resource_value = header
            .iter()
            .any(|record| record.parameters.iter().any(contains_resource_value))
            || records.values().any(|record| {
                record
                    .partials
                    .iter()
                    .any(|partial| partial.parameters.iter().any(contains_resource_value))
            });
        if has_resource_value {
            return self.err("resource values are only valid in edition-3 anchor items");
        }
        if let Some(offset) = self.first_omitted_entity_name_offset {
            self.diagnostics.push(ParseDiagnostic {
                offset,
                kind: ParseDiagnosticKind::OmittedEntityName,
                message: format!(
                    "recovered {} simple named carrier instance(s) with an omitted leading name attribute by inserting an empty name",
                    self.omitted_entity_name_count
                ),
            });
        }
        for capacity in [
            compact_vec(&mut header),
            compact_vec(&mut anchors),
            compact_vec(&mut reference_entries),
            compact_vec(&mut data),
            compact_vec(&mut signatures),
            compact_vec(&mut signature_sections),
        ] {
            self.charge_retained(capacity, "step_parse_exchange_storage")?;
        }
        Ok((
            Exchange {
                header,
                anchors,
                references: reference_entries,
                data,
                signatures,
                signature_sections,
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
        self.charge_vec_storage(&partials, "step_parse_record_storage")?;
        self.punct(&TokenKind::Semicolon)?;
        Ok(RawRecord {
            id,
            partials,
            span: start..self.previous_end(),
        })
    }

    fn partial(&mut self) -> Result<PartialRecord, ParseError> {
        let name = self.take_name()?;
        self.charge_string_storage(&name, "step_parse_name_storage")?;
        let parameters = self.parameters()?;
        self.charge_value_vec_storage(&parameters, "step_parse_collection_storage")?;
        Ok(PartialRecord { name, parameters })
    }

    fn parameters(&mut self) -> Result<Vec<Value>, ParseError> {
        const MAX_VALUE_DEPTH: usize = 256;
        let budget = self.budget;
        let _nested = budget
            .map(|ctx| ctx.enter_nested("step_parse_parameter_nesting", None))
            .transpose()
            .map_err(ParseError::Resource)?;
        if self.depth >= recursion_cap(budget, MAX_VALUE_DEPTH) {
            return self.err("parameter nesting exceeds 256 levels");
        }
        self.depth += 1;
        let result = self.parameters_inner();
        self.depth -= 1;
        result.map(|mut values| {
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
            Value::List(self.parameters()?)
        } else {
            match self.next_kind()? {
                TokenKind::Instance(v) => Value::Reference(v),
                TokenKind::ValueInstance(v) => Value::ValueReference(v),
                TokenKind::ConstantEntity(mut name) => {
                    name.shrink_to_fit();
                    Value::ConstantEntity(name)
                }
                TokenKind::ConstantValue(mut name) => {
                    name.shrink_to_fit();
                    Value::ConstantValue(name)
                }
                TokenKind::Integer(v) => Value::Integer(v),
                TokenKind::Real(v) => Value::Real(v),
                TokenKind::Enumeration(mut value) => {
                    value.shrink_to_fit();
                    Value::Enumeration(value)
                }
                TokenKind::String(mut value) => {
                    value.shrink_to_fit();
                    Value::String(value)
                }
                TokenKind::Binary(mut value) => {
                    value.data.shrink_to_fit();
                    Value::Binary(value)
                }
                TokenKind::Resource(mut value) => {
                    value.shrink_to_fit();
                    Value::Resource(value)
                }
                TokenKind::Omitted => Value::Omitted,
                TokenKind::Derived => Value::Derived,
                TokenKind::Name(name) => self.typed_parameter(name)?,
                TokenKind::UserName(name) => self.typed_parameter(format!("!{name}"))?,
                _ => return self.err("expected parameter value"),
            }
        };
        self.charge_retained(value_node_storage_bytes(&value), "step_parse_value_storage")?;
        Ok(value)
    }

    fn typed_parameter(&mut self, mut name: String) -> Result<Value, ParseError> {
        name.shrink_to_fit();
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

    fn take_name(&mut self) -> Result<String, ParseError> {
        match self.next_kind()? {
            TokenKind::Name(name) => Ok(name),
            TokenKind::UserName(name) => Ok(format!("!{name}")),
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

fn allocation_bytes(capacity: usize, element_size: usize) -> u64 {
    u64::try_from(capacity)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::try_from(element_size).unwrap_or(u64::MAX))
}

fn compact_vec<T>(values: &mut Vec<T>) -> u64 {
    values.shrink_to_fit();
    allocation_bytes(values.capacity(), size_of::<T>())
}

fn btree_node_storage<K, V>() -> u64 {
    allocation_bytes(
        1,
        size_of::<(K, V)>().saturating_add(3 * size_of::<usize>()),
    )
}

fn value_node_storage_bytes(value: &Value) -> u64 {
    let dynamic = match value {
        Value::ConstantEntity(value)
        | Value::ConstantValue(value)
        | Value::Enumeration(value)
        | Value::Resource(value) => value.capacity(),
        Value::String(value) => value.capacity(),
        Value::Binary(value) => value.data.len(),
        Value::List(values) => values.capacity().saturating_mul(size_of::<Value>()),
        Value::Typed(name, _) => name.capacity().saturating_add(size_of::<Value>()),
        Value::Reference(_)
        | Value::ValueReference(_)
        | Value::Integer(_)
        | Value::Real(_)
        | Value::Omitted
        | Value::Derived => 0,
    };
    u64::try_from(size_of::<Value>())
        .unwrap_or(u64::MAX)
        .saturating_add(u64::try_from(dynamic).unwrap_or(u64::MAX))
}

fn value_storage_bytes(value: &Value) -> u64 {
    value_node_storage_bytes(value).saturating_add(match value {
        Value::List(values) => values
            .iter()
            .map(value_storage_bytes)
            .fold(0, u64::saturating_add),
        Value::Typed(_, value) => value_storage_bytes(value),
        _ => 0,
    })
}

fn validate_header(header: &[HeaderRecord]) -> Result<ImplementationLevel, &'static str> {
    const REQUIRED: [&str; 3] = ["FILE_DESCRIPTION", "FILE_NAME", "FILE_SCHEMA"];
    if header.len() < REQUIRED.len()
        || header
            .iter()
            .zip(REQUIRED)
            .any(|(record, expected)| record.name != expected)
    {
        return Err("HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA");
    }
    if REQUIRED
        .iter()
        .any(|name| header.iter().filter(|record| record.name == *name).count() != 1)
    {
        return Err("HEADER contains a duplicate required entity");
    }

    let description = &header[0].parameters;
    if description.len() != 2
        || !is_string_list(description.first())
        || !matches!(description.get(1), Some(Value::String(_)))
    {
        return Err("FILE_DESCRIPTION has invalid parameters");
    }
    let implementation_level = match description.get(1) {
        Some(Value::String(value)) => {
            let Ok(level) = crate::strings::decode(value) else {
                return Err("FILE_DESCRIPTION has an unsupported implementation level");
            };
            match level.as_str() {
                "1" | "2" | "2;1" | "2;2" => ImplementationLevel::LegacyEdition1,
                "3;1" | "3;2" => ImplementationLevel::LegacyEdition2,
                "4;1" => ImplementationLevel::Edition3Class1,
                "4;2" => ImplementationLevel::Edition3Class2,
                "4;3" => ImplementationLevel::Edition3Class3,
                _ => return Err("FILE_DESCRIPTION has an unsupported implementation level"),
            }
        }
        _ => return Err("FILE_DESCRIPTION has invalid parameters"),
    };
    if !is_decodable_string_list(description.first(), implementation_level)
        || !is_decodable_string(
            description.get(1).expect("FILE_DESCRIPTION has two values"),
            implementation_level,
        )
    {
        return Err("FILE_DESCRIPTION has invalid string encoding");
    }
    if !string_list_within_limit(description.first(), implementation_level, 256)
        || !string_within_limit(
            description.get(1).expect("FILE_DESCRIPTION has two values"),
            implementation_level,
            256,
        )
    {
        return Err("FILE_DESCRIPTION contains a string longer than 256 characters");
    }

    let file_name = &header[1].parameters;
    // Producer metadata after the author and organization lists may be unset.
    if file_name.len() != 7
        || !matches!(file_name.first(), Some(Value::String(_)))
        || !matches!(file_name.get(1), Some(Value::String(_)))
        || !is_string_list(file_name.get(2))
        || !is_string_list(file_name.get(3))
        || !is_string_or_omitted(file_name.get(4))
        || !is_string_or_omitted(file_name.get(5))
        || !is_string_or_omitted(file_name.get(6))
    {
        return Err("FILE_NAME has invalid parameters");
    }
    if !is_decodable_string(
        file_name.first().expect("FILE_NAME has seven values"),
        implementation_level,
    ) || !is_decodable_string(
        file_name.get(1).expect("FILE_NAME has seven values"),
        implementation_level,
    ) || !is_decodable_string_list(file_name.get(2), implementation_level)
        || !is_decodable_string_list(file_name.get(3), implementation_level)
        || !is_decodable_string_or_omitted(
            file_name.get(4).expect("FILE_NAME has seven values"),
            implementation_level,
        )
        || !is_decodable_string_or_omitted(
            file_name.get(5).expect("FILE_NAME has seven values"),
            implementation_level,
        )
        || !is_decodable_string_or_omitted(
            file_name.get(6).expect("FILE_NAME has seven values"),
            implementation_level,
        )
    {
        return Err("FILE_NAME has invalid string encoding");
    }
    if !string_within_limit(
        file_name.first().expect("FILE_NAME has seven values"),
        implementation_level,
        256,
    ) || !string_within_limit(
        file_name.get(1).expect("FILE_NAME has seven values"),
        implementation_level,
        256,
    ) || !string_list_within_limit(file_name.get(2), implementation_level, 256)
        || !string_list_within_limit(file_name.get(3), implementation_level, 256)
        || !string_or_omitted_within_limit(
            file_name.get(4).expect("FILE_NAME has seven values"),
            implementation_level,
            256,
        )
        || !string_or_omitted_within_limit(
            file_name.get(5).expect("FILE_NAME has seven values"),
            implementation_level,
            256,
        )
        || !string_or_omitted_within_limit(
            file_name.get(6).expect("FILE_NAME has seven values"),
            implementation_level,
            256,
        )
    {
        return Err("FILE_NAME contains a string longer than 256 characters");
    }
    let time_stamp = decoded_string(
        file_name.get(1).expect("FILE_NAME has seven values"),
        implementation_level,
    )
    .expect("FILE_NAME timestamp was checked as decodable");
    if !time_stamp.is_empty() && !valid_timestamp_text(&time_stamp) {
        return Err("FILE_NAME has an invalid timestamp");
    }

    let schema = &header[2].parameters;
    let Some(Value::List(identifiers)) = schema.first() else {
        return Err("FILE_SCHEMA must contain one schema identifier list");
    };
    if schema.len() != 1
        || identifiers.is_empty()
        || !identifiers
            .iter()
            .all(|value| matches!(value, Value::String(_)))
    {
        return Err("FILE_SCHEMA has invalid or duplicate schema identifiers");
    }
    let mut normalized_identifiers = BTreeSet::new();
    for value in identifiers {
        let Value::String(bytes) = value else {
            unreachable!("FILE_SCHEMA identifiers were checked as strings");
        };
        let Ok(identifier) = decode_string(bytes, implementation_level) else {
            return Err("FILE_SCHEMA has invalid or duplicate schema identifiers");
        };
        if !valid_schema_identifier(&identifier) {
            return Err("FILE_SCHEMA has invalid or duplicate schema identifiers");
        }
        if !normalized_identifiers.insert(identifier.to_ascii_uppercase()) {
            return Err("FILE_SCHEMA has invalid or duplicate schema identifiers");
        }
    }
    Ok(implementation_level)
}

fn validate_header_sections(
    implementation_level: ImplementationLevel,
    header: &[HeaderRecord],
    schema_identifiers: &[String],
) -> Result<(), &'static str> {
    let has = |name: &str| header.iter().any(|record| record.name == name);
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("FILE_POPULATION") {
        return Err("2;1 forbids FILE_POPULATION in HEADER");
    }
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("SECTION_LANGUAGE") {
        return Err("2;1 forbids SECTION_LANGUAGE in HEADER");
    }
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("SECTION_CONTEXT") {
        return Err("2;1 forbids SECTION_CONTEXT in HEADER");
    }
    if matches!(
        implementation_level,
        ImplementationLevel::LegacyEdition2 | ImplementationLevel::Edition3Class1
    ) && has("SCHEMA_POPULATION")
    {
        return Err(match implementation_level {
            ImplementationLevel::LegacyEdition2 => "3;1 forbids SCHEMA_POPULATION in HEADER",
            ImplementationLevel::Edition3Class1 => "4;1 forbids SCHEMA_POPULATION in HEADER",
            ImplementationLevel::LegacyEdition1
            | ImplementationLevel::Edition3Class2
            | ImplementationLevel::Edition3Class3 => unreachable!(),
        });
    }

    let mut user_defined = false;
    let mut schema_population_seen = false;
    let mut language_sections = BTreeSet::new();
    let mut context_sections = BTreeSet::new();
    for record in header.iter().skip(3) {
        if record.name.starts_with('!') {
            user_defined = true;
            continue;
        }
        if user_defined {
            return Err("built-in HEADER entities must precede user-defined entities");
        }
        match record.name.as_str() {
            "SCHEMA_POPULATION" => {
                if schema_population_seen {
                    return Err("HEADER contains duplicate SCHEMA_POPULATION");
                }
                schema_population_seen = true;
                if !valid_schema_population(&record.parameters, implementation_level) {
                    return Err("SCHEMA_POPULATION has invalid parameters");
                }
            }
            "FILE_POPULATION" => {
                if !valid_file_population(
                    &record.parameters,
                    schema_identifiers,
                    implementation_level,
                ) {
                    return Err("FILE_POPULATION has invalid parameters");
                }
            }
            "SECTION_LANGUAGE" => {
                let section = valid_section_language(&record.parameters, implementation_level)
                    .map_err(|()| "SECTION_LANGUAGE has invalid parameters")?;
                if !language_sections.insert(section) {
                    return Err("HEADER contains duplicate SECTION_LANGUAGE section");
                }
            }
            "SECTION_CONTEXT" => {
                let section = valid_section_context(&record.parameters, implementation_level)
                    .map_err(|()| "SECTION_CONTEXT has invalid parameters")?;
                if !context_sections.insert(section) {
                    return Err("HEADER contains duplicate SECTION_CONTEXT section");
                }
            }
            _ => return Err("HEADER contains an unsupported entity"),
        }
    }
    Ok(())
}

fn valid_schema_population(
    parameters: &[Value],
    implementation_level: ImplementationLevel,
) -> bool {
    let [Value::List(identifications)] = parameters else {
        return false;
    };
    !identifications.is_empty()
        && identifications.iter().all(|identification| {
            let Value::List(values) = identification else {
                return false;
            };
            let [Value::String(address), time_stamp, digest] = values.as_slice() else {
                return false;
            };
            decoded_bytes(address, implementation_level).is_some()
                && valid_optional_timestamp(time_stamp, implementation_level)
                && valid_optional_base64(digest, implementation_level)
        })
}

fn valid_file_population(
    parameters: &[Value],
    schema_identifiers: &[String],
    implementation_level: ImplementationLevel,
) -> bool {
    let [Value::String(schema), Value::String(determination), governed_sections] = parameters
    else {
        return false;
    };
    let Some(schema) = decoded_bytes(schema, implementation_level) else {
        return false;
    };
    if !valid_schema_identifier(&schema)
        || decoded_bytes(determination, implementation_level).is_none()
    {
        return false;
    }
    if !schema_identifier_matches(schema_identifiers, &schema) {
        return false;
    }
    match governed_sections {
        Value::Omitted => true,
        Value::List(sections) if !sections.is_empty() => {
            let mut names = BTreeSet::new();
            sections.iter().all(|section| {
                let Some(section) = decoded_string(section, implementation_level) else {
                    return false;
                };
                names.insert(section)
            })
        }
        _ => false,
    }
}

fn valid_section_language(
    parameters: &[Value],
    implementation_level: ImplementationLevel,
) -> Result<Option<String>, ()> {
    let [section, language] = parameters else {
        return Err(());
    };
    let Some(language) = decoded_string(language, implementation_level) else {
        return Err(());
    };
    if language.len() != 3 || !language.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(());
    }
    valid_optional_section_name(section, implementation_level)
}

fn valid_section_context(
    parameters: &[Value],
    implementation_level: ImplementationLevel,
) -> Result<Option<String>, ()> {
    let [section, Value::List(contexts)] = parameters else {
        return Err(());
    };
    if contexts.is_empty()
        || !contexts
            .iter()
            .all(|context| decoded_string(context, implementation_level).is_some())
    {
        return Err(());
    }
    valid_optional_section_name(section, implementation_level)
}

fn valid_optional_section_name(
    value: &Value,
    implementation_level: ImplementationLevel,
) -> Result<Option<String>, ()> {
    match value {
        Value::Omitted => Ok(None),
        value => decoded_string(value, implementation_level)
            .map(Some)
            .ok_or(()),
    }
}

fn is_decodable_string(value: &Value, implementation_level: ImplementationLevel) -> bool {
    decoded_string(value, implementation_level).is_some()
}

fn is_decodable_string_list(
    value: Option<&Value>,
    implementation_level: ImplementationLevel,
) -> bool {
    matches!(
        value,
        Some(Value::List(values))
            if !values.is_empty()
                && values
                    .iter()
                    .all(|value| is_decodable_string(value, implementation_level))
    )
}

fn is_decodable_string_or_omitted(
    value: &Value,
    implementation_level: ImplementationLevel,
) -> bool {
    matches!(value, Value::Omitted) || is_decodable_string(value, implementation_level)
}

fn string_within_limit(
    value: &Value,
    implementation_level: ImplementationLevel,
    limit: usize,
) -> bool {
    decoded_string(value, implementation_level).is_some_and(|value| value.chars().count() <= limit)
}

fn string_list_within_limit(
    value: Option<&Value>,
    implementation_level: ImplementationLevel,
    limit: usize,
) -> bool {
    matches!(
        value,
        Some(Value::List(values))
            if !values.is_empty()
                && values.iter().all(|value| {
                    string_within_limit(value, implementation_level, limit)
                })
    )
}

fn string_or_omitted_within_limit(
    value: &Value,
    implementation_level: ImplementationLevel,
    limit: usize,
) -> bool {
    matches!(value, Value::Omitted) || string_within_limit(value, implementation_level, limit)
}

fn valid_optional_timestamp(value: &Value, implementation_level: ImplementationLevel) -> bool {
    match value {
        Value::Omitted => true,
        Value::String(_) => decoded_string(value, implementation_level)
            .is_some_and(|value| valid_timestamp_text(&value)),
        _ => false,
    }
}

fn valid_timestamp_text(value: &str) -> bool {
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
        return false;
    }
    let year = parse_ascii_digits(&bytes[0..4]);
    let month = parse_ascii_digits(&bytes[5..7]);
    let day = parse_ascii_digits(&bytes[8..10]);
    let hour = parse_ascii_digits(&bytes[11..13]);
    let minute = parse_ascii_digits(&bytes[14..16]);
    let second = parse_ascii_digits(&bytes[17..19]);
    let Some(days) = days_in_month(year, month) else {
        return false;
    };
    if day == 0
        || day > days
        || minute > 59
        || second > 60
        || hour > 24
        || (hour == 24 && (minute != 0 || second != 0))
    {
        return false;
    }

    let mut at = 19;
    if matches!(bytes.get(at), Some(b'.' | b',')) {
        at += 1;
        let fraction_start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == fraction_start {
            return false;
        }
    }
    match bytes.get(at).copied() {
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
    }
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

fn all_ascii_digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

fn parse_ascii_digits(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + usize::from(byte - b'0'))
}

fn valid_optional_base64(value: &Value, implementation_level: ImplementationLevel) -> bool {
    match value {
        Value::Omitted => true,
        Value::String(_) => decoded_string(value, implementation_level)
            .is_some_and(|value| valid_base64_text(value.as_bytes())),
        _ => false,
    }
}

fn valid_base64_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let mut quantum_len = 0;
    let mut padding = 0;
    let mut finished = false;
    for &byte in bytes {
        let is_alphabet = byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/');
        if finished || (padding != 0 && is_alphabet) {
            return false;
        }
        if is_alphabet {
            quantum_len += 1;
        } else if byte == b'=' {
            if quantum_len < 2 || padding == 2 {
                return false;
            }
            padding += 1;
            quantum_len += 1;
        } else {
            return false;
        }
        if quantum_len == 4 {
            finished = padding != 0;
            quantum_len = 0;
            padding = 0;
        }
    }
    quantum_len == 0
}

fn decode_signature_payload(input: &[u8], payload: &Range<usize>) -> Result<Vec<u8>, ParseError> {
    let compact = input[payload.clone()]
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_control() && *byte != b' ')
        .collect::<Vec<_>>();
    let cms = STANDARD
        .decode(compact)
        .map_err(|error| ParseError::Syntax {
            offset: payload.start,
            message: format!("invalid SIGNATURE Base64 payload: {error}"),
        })?;
    crate::signature::validate_detached_cms(&cms).map_err(|message| ParseError::Syntax {
        offset: payload.start,
        message: format!("invalid detached CMS SIGNATURE payload: {message}"),
    })?;
    Ok(cms)
}

fn valid_schema_identifier(identifier: &str) -> bool {
    let identifier = identifier.trim().to_ascii_uppercase();
    if identifier.is_empty() || identifier.chars().count() > 1024 {
        return false;
    }
    let (name, object_identifier) = match identifier.split_once('{') {
        Some((name, object_identifier)) => {
            let Some(object_identifier) = object_identifier.strip_suffix('}') else {
                return false;
            };
            (name.trim_end(), Some(object_identifier))
        }
        None => (identifier.as_str(), None),
    };
    if name.is_empty()
        || name.trim() != name
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return false;
    }
    object_identifier.is_none_or(|value| {
        let mut components = value.split_whitespace();
        components.next().is_some() && value.split_whitespace().all(valid_schema_oid_component)
    })
}

fn valid_schema_oid_component(component: &str) -> bool {
    let digits = component
        .strip_prefix('-')
        .or_else(|| component.strip_prefix('+'))
        .unwrap_or(component);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn decoded_string(value: &Value, implementation_level: ImplementationLevel) -> Option<String> {
    let Value::String(bytes) = value else {
        return None;
    };
    decoded_bytes(bytes, implementation_level)
}

fn decoded_bytes(bytes: &[u8], implementation_level: ImplementationLevel) -> Option<String> {
    decode_string(bytes, implementation_level).ok()
}

fn schema_identifier_matches(schema_identifiers: &[String], schema_name: &str) -> bool {
    let schema_name = schema_name.trim().to_ascii_uppercase();
    schema_identifiers.iter().any(|identifier| {
        let identifier = identifier.trim();
        identifier == schema_name || schema_name_without_oid(identifier) == schema_name
    })
}

fn validate_header_data_references(
    header: &[HeaderRecord],
    data: &[DataSection],
    implementation_level: ImplementationLevel,
) -> Result<(), &'static str> {
    let mut names = BTreeSet::new();
    for section in data {
        if let [Value::String(name), Value::List(_)] = section.parameters.as_slice() {
            let name = decoded_string(&Value::String(name.clone()), implementation_level)
                .ok_or("DATA section parameters contain an invalid string")?;
            names.insert(name);
        }
    }
    for record in header.iter().skip(3) {
        match record.name.as_str() {
            "FILE_POPULATION" => {
                let Some(Value::List(sections)) = record.parameters.get(2) else {
                    continue;
                };
                for section in sections {
                    let section = decoded_string(section, implementation_level)
                        .ok_or("FILE_POPULATION has invalid parameters")?;
                    if !names.contains(&section) {
                        return Err("FILE_POPULATION names an unknown DATA section");
                    }
                }
            }
            "SECTION_LANGUAGE" => {
                validate_header_section_name(&record.parameters[0], &names, implementation_level)?;
            }
            "SECTION_CONTEXT" => {
                validate_header_section_name(&record.parameters[0], &names, implementation_level)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_header_section_name(
    value: &Value,
    data_section_names: &BTreeSet<String>,
    implementation_level: ImplementationLevel,
) -> Result<(), &'static str> {
    let Value::Omitted = value else {
        let section = decoded_string(value, implementation_level)
            .ok_or("header section reference has invalid parameters")?;
        if !data_section_names.contains(&section) {
            return Err("header section reference names an unknown DATA section");
        }
        return Ok(());
    };
    Ok(())
}

fn schema_identifier_count(header: &[HeaderRecord]) -> usize {
    match header[2].parameters.first() {
        Some(Value::List(identifiers)) => identifiers.len(),
        _ => 0,
    }
}

fn valid_data_parameters(
    parameters: &[Value],
    schema_identifiers: &[String],
    implementation_level: ImplementationLevel,
    section_names: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    let [Value::String(section_name), Value::List(schema)] = parameters else {
        return Err("DATA section parameters must contain a name and one schema");
    };
    let [Value::String(schema_name)] = schema.as_slice() else {
        return Err("DATA section parameters must contain a name and one schema");
    };
    let section_name = decode_string(section_name, implementation_level)
        .map_err(|_| "DATA section parameters contain an invalid string")?;
    if !section_names.insert(section_name) {
        return Err("DATA section names must be unique");
    }
    let schema_name = decode_string(schema_name, implementation_level)
        .map_err(|_| "DATA section parameters contain an invalid string")?;
    if !valid_schema_identifier(&schema_name)
        || !schema_identifier_matches(schema_identifiers, &schema_name)
    {
        return Err("DATA section schema is not listed in FILE_SCHEMA");
    }
    Ok(())
}

fn schema_name_without_oid(identifier: &str) -> &str {
    identifier
        .trim()
        .split_once('{')
        .map_or_else(|| identifier.trim(), |(name, _)| name.trim())
}

fn schema_identifiers(
    header: &[HeaderRecord],
    implementation_level: ImplementationLevel,
) -> Vec<String> {
    let Some(Value::List(identifiers)) = header[2].parameters.first() else {
        return Vec::new();
    };
    identifiers
        .iter()
        .filter_map(|value| match value {
            Value::String(bytes) => decode_string(bytes, implementation_level).ok(),
            _ => None,
        })
        .map(|identifier| identifier.to_ascii_uppercase())
        .collect()
}

fn decode_string(
    bytes: &[u8],
    implementation_level: ImplementationLevel,
) -> Result<String, crate::strings::StringError> {
    match implementation_level {
        ImplementationLevel::LegacyEdition1 | ImplementationLevel::LegacyEdition2 => {
            crate::strings::decode(bytes)
        }
        ImplementationLevel::Edition3Class1
        | ImplementationLevel::Edition3Class2
        | ImplementationLevel::Edition3Class3 => crate::strings::decode_utf8(bytes),
    }
}

fn is_string_list(value: Option<&Value>) -> bool {
    matches!(
        value,
        Some(Value::List(values))
            if !values.is_empty() && values.iter().all(|value| matches!(value, Value::String(_)))
    )
}

fn is_string_or_omitted(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::String(_) | Value::Omitted))
}

fn valid_anchor_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().any(|byte| !byte.is_ascii_digit())
}

fn is_anchor_item(value: &Value) -> bool {
    match value {
        Value::Reference(_)
        | Value::ValueReference(_)
        | Value::ConstantEntity(_)
        | Value::ConstantValue(_)
        | Value::Integer(_)
        | Value::Real(_)
        | Value::Enumeration(_)
        | Value::String(_)
        | Value::Binary(_)
        | Value::Resource(_)
        | Value::Omitted => true,
        Value::List(values) => values.iter().all(is_anchor_item),
        Value::Derived | Value::Typed(_, _) => false,
    }
}

#[derive(Debug)]
enum ResolveError {
    Syntax(String),
    Resource(CodecError),
}

impl ResolveError {
    fn into_parse_error(self, offset: usize) -> ParseError {
        match self {
            Self::Syntax(message) => ParseError::Syntax { offset, message },
            Self::Resource(error) => ParseError::Resource(error),
        }
    }
}

impl From<String> for ResolveError {
    fn from(message: String) -> Self {
        Self::Syntax(message)
    }
}

impl From<&str> for ResolveError {
    fn from(message: &str) -> Self {
        Self::Syntax(message.into())
    }
}

fn collection_cap(budget: Option<&DecodeContext<'_>>, format_cap: usize) -> usize {
    budget
        .and_then(|ctx| usize::try_from(ctx.policy().limits.max_collection_items).ok())
        .map_or(format_cap, |policy| policy.min(format_cap))
}

fn recursion_cap(budget: Option<&DecodeContext<'_>>, format_cap: usize) -> usize {
    budget
        .and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(format_cap, |policy| policy.min(format_cap))
}

struct AnchorResolver<'a, 'ctx, 'arena> {
    anchors: &'a BTreeMap<String, Value>,
    memo: BTreeMap<String, (Value, usize)>,
    remaining_nodes: usize,
    budget: Option<&'ctx DecodeContext<'arena>>,
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
            remaining_nodes: collection_cap(budget, Self::MAX_EXPANDED_NODES),
            budget,
        }
    }

    fn resolve_root(&mut self, value: &Value) -> Result<Value, ResolveError> {
        let (value, _, expanded_nodes) =
            self.resolve(value, &mut Vec::new(), self.remaining_nodes, 0)?;
        self.remaining_nodes = self
            .remaining_nodes
            .checked_sub(expanded_nodes)
            .ok_or_else(|| {
                ResolveError::Syntax("aggregate expanded anchor graph exceeds 1000000 nodes".into())
            })?;
        Ok(value)
    }

    fn charge_nodes(&self, count: usize) -> Result<(), ResolveError> {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        if let Some(budget) = self.budget {
            budget
                .charge_collection_items(count, "step_anchor_materialization")
                .map_err(ResolveError::Resource)?;
            budget
                .charge_work(count, "step_anchor_materialization")
                .map_err(ResolveError::Resource)?;
        }
        Ok(())
    }

    fn charge_storage(&self, value: &Value) -> Result<(), ResolveError> {
        if let Some(budget) = self.budget {
            budget
                .charge_retained(
                    value_storage_bytes(value),
                    "step_anchor_materialization_storage",
                    None,
                )
                .map_err(ResolveError::Resource)?;
        }
        Ok(())
    }

    fn resolve(
        &mut self,
        value: &Value,
        stack: &mut Vec<String>,
        budget: usize,
        depth: usize,
    ) -> Result<(Value, usize, usize), ResolveError> {
        let _nested = self
            .budget
            .map(|ctx| ctx.enter_nested("step_anchor_reference", None))
            .transpose()
            .map_err(ResolveError::Resource)?;
        if depth >= recursion_cap(self.budget, Self::MAX_REFERENCE_DEPTH) {
            return Err("expanded anchor graph exceeds its node or depth limit".into());
        }
        match value {
            Value::Resource(name) if self.anchors.contains_key(name) => {
                if let Some((value, nodes)) = self.memo.get(name) {
                    if *nodes > budget {
                        return Err("expanded anchor value exceeds 1000000 nodes".into());
                    }
                    self.charge_nodes(*nodes)?;
                    self.charge_storage(value)?;
                    return Ok((value.clone(), *nodes, *nodes));
                }
                if stack.contains(name) {
                    return Err(format!("cyclic anchor binding <{name}>").into());
                }
                stack.push(name.clone());
                let source = &self.anchors[name];
                self.charge_nodes(value_node_count(source, Self::MAX_EXPANDED_NODES)?)?;
                self.charge_storage(source)?;
                let source = source.clone();
                let resolved = self.resolve(&source, stack, budget, depth + 1);
                stack.pop();
                let (value, nodes, _) = resolved?;
                if nodes > budget {
                    return Err("expanded anchor value exceeds 1000000 nodes".into());
                }
                self.charge_nodes(nodes)?;
                self.charge_storage(&value)?;
                self.memo.insert(name.clone(), (value.clone(), nodes));
                self.charge_storage(&value)?;
                Ok((value, nodes, nodes))
            }
            Value::List(values) => {
                self.charge_nodes(1)?;
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
                let resolved = Value::List(resolved);
                self.charge_storage(&resolved)?;
                Ok((resolved, nodes, expanded_nodes))
            }
            Value::Typed(name, value) => {
                self.charge_nodes(1)?;
                let (value, nodes, expanded_nodes) =
                    self.resolve(value, stack, budget, depth + 1)?;
                let value = Value::Typed(name.clone(), Box::new(value));
                self.charge_storage(&value)?;
                Ok((value, nodes + 1, expanded_nodes))
            }
            value => {
                self.charge_nodes(1)?;
                self.charge_storage(value)?;
                Ok((value.clone(), 1, 0))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReferenceKind {
    Entity,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ReferenceKey {
    kind: ReferenceKind,
    id: u64,
}

struct ReferenceResolver<'a, 'ctx, 'arena> {
    bindings: BTreeMap<ReferenceKey, &'a str>,
    anchors: &'a BTreeMap<String, Value>,
    stack: Vec<ReferenceKey>,
    remaining_nodes: usize,
    budget: Option<&'ctx DecodeContext<'arena>>,
}

impl<'a, 'ctx, 'arena> ReferenceResolver<'a, 'ctx, 'arena> {
    const MAX_MATERIALIZED_NODES: usize = 1_000_000;
    const MAX_REFERENCE_DEPTH: usize = 256;

    fn new(
        references: &'a [ReferenceEntry],
        anchors: &'a BTreeMap<String, Value>,
        budget: Option<&'ctx DecodeContext<'arena>>,
    ) -> Result<Self, ResolveError> {
        let mut bindings = BTreeMap::new();
        for reference in references {
            let (kind, id) = match reference.name.as_bytes().first() {
                Some(b'#') => (ReferenceKind::Entity, &reference.name[1..]),
                Some(b'@') => (ReferenceKind::Value, &reference.name[1..]),
                _ => {
                    return Err(ResolveError::Syntax(
                        "invalid REFERENCE occurrence name".into(),
                    ))
                }
            };
            let id = id
                .parse::<u64>()
                .map_err(|_| ResolveError::Syntax("invalid REFERENCE occurrence name".into()))?;
            if bindings
                .insert(ReferenceKey { kind, id }, reference.uri.as_str())
                .is_some()
            {
                return Err(ResolveError::Syntax(
                    "duplicate REFERENCE occurrence name".into(),
                ));
            }
        }
        Ok(Self {
            bindings,
            anchors,
            stack: Vec::new(),
            remaining_nodes: collection_cap(budget, Self::MAX_MATERIALIZED_NODES),
            budget,
        })
    }

    fn resolve_value(&mut self, value: &Value, depth: usize) -> Result<Value, ResolveError> {
        let _nested = self
            .budget
            .map(|ctx| ctx.enter_nested("step_reference_expansion", None))
            .transpose()
            .map_err(ResolveError::Resource)?;
        if depth >= recursion_cap(self.budget, Self::MAX_REFERENCE_DEPTH) {
            return Err("REFERENCE expansion exceeds its depth limit".into());
        }
        match value {
            Value::Reference(id) => self.resolve_occurrence(
                ReferenceKey {
                    kind: ReferenceKind::Entity,
                    id: *id,
                },
                value,
                depth,
            ),
            Value::ValueReference(id) => self.resolve_occurrence(
                ReferenceKey {
                    kind: ReferenceKind::Value,
                    id: *id,
                },
                value,
                depth,
            ),
            Value::List(values) => values
                .iter()
                .map(|value| self.resolve_value(value, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            Value::Typed(name, value) => Ok(Value::Typed(
                name.clone(),
                Box::new(self.resolve_value(value, depth + 1)?),
            )),
            _ => Ok(value.clone()),
        }
    }

    fn resolve_occurrence(
        &mut self,
        key: ReferenceKey,
        original: &Value,
        depth: usize,
    ) -> Result<Value, ResolveError> {
        let Some(uri) = self.bindings.get(&key).copied() else {
            return Ok(original.clone());
        };
        let Some((path, fragment)) = uri.split_once('#') else {
            return Ok(Value::Omitted);
        };
        if !path.is_empty() {
            return Ok(original.clone());
        }
        if self.stack.contains(&key) {
            return Ok(Value::Omitted);
        }
        let Some(anchor) = self.anchors.get(fragment) else {
            return if is_uuid_fragment(fragment) {
                Ok(original.clone())
            } else {
                Ok(Value::Omitted)
            };
        };
        self.stack.push(key);
        let resolved = self.resolve_value(anchor, depth + 1);
        self.stack.pop();
        let resolved = resolved?;
        if let Value::Resource(uri) = &resolved {
            return if uri.contains('#') {
                Ok(original.clone())
            } else {
                Ok(Value::Omitted)
            };
        }
        if !reference_target_matches(key.kind, &resolved) {
            return Ok(Value::Omitted);
        }
        self.charge_materialized_nodes(&resolved)?;
        Ok(resolved)
    }

    fn charge_materialized_nodes(&mut self, value: &Value) -> Result<(), ResolveError> {
        let nodes = value_node_count(value, self.remaining_nodes)?;
        self.remaining_nodes = self.remaining_nodes.checked_sub(nodes).ok_or_else(|| {
            ResolveError::Syntax("REFERENCE expansion exceeds 1000000 nodes".into())
        })?;
        if let Some(budget) = self.budget {
            let nodes = u64::try_from(nodes).unwrap_or(u64::MAX);
            budget
                .charge_collection_items(nodes, "step_reference_materialization")
                .map_err(ResolveError::Resource)?;
            budget
                .charge_work(nodes, "step_reference_materialization")
                .map_err(ResolveError::Resource)?;
            budget
                .charge_retained(
                    value_storage_bytes(value),
                    "step_reference_materialization_storage",
                    None,
                )
                .map_err(ResolveError::Resource)?;
        }
        Ok(())
    }
}

fn resolve_local_references(
    anchors: &mut [AnchorEntry],
    records: &mut BTreeMap<u64, RawRecord>,
    references: &[ReferenceEntry],
    budget: Option<&DecodeContext<'_>>,
) -> Result<(), ResolveError> {
    if references.is_empty() {
        return Ok(());
    }
    let anchor_bindings = anchors
        .iter()
        .map(|anchor| (anchor.name.clone(), anchor.value.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut resolver = ReferenceResolver::new(references, &anchor_bindings, budget)?;
    for anchor in anchors {
        anchor.value = resolver.resolve_value(&anchor.value, 0)?;
        for tag in &mut anchor.tags {
            tag.value = resolver.resolve_value(&tag.value, 0)?;
        }
    }
    for record in records.values_mut() {
        for partial in &mut record.partials {
            for value in &mut partial.parameters {
                *value = resolver.resolve_value(value, 0)?;
            }
        }
    }
    Ok(())
}

fn reference_target_matches(kind: ReferenceKind, value: &Value) -> bool {
    match kind {
        ReferenceKind::Entity => matches!(value, Value::Reference(_) | Value::ConstantEntity(_)),
        ReferenceKind::Value => !matches!(
            value,
            Value::Reference(_) | Value::ConstantEntity(_) | Value::Resource(_)
        ),
    }
}

fn is_uuid_fragment(fragment: &str) -> bool {
    fragment.len() == 36
        && fragment.as_bytes().iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                .then_some(*byte == b'-')
                .unwrap_or_else(|| byte.is_ascii_hexdigit())
        })
}

fn value_node_count(value: &Value, limit: usize) -> Result<usize, String> {
    let mut pending = vec![value];
    let mut count = 0usize;
    while let Some(value) = pending.pop() {
        count = count
            .checked_add(1)
            .ok_or_else(|| "REFERENCE expansion exceeds 1000000 nodes".to_string())?;
        if count > limit {
            return Err("REFERENCE expansion exceeds 1000000 nodes".into());
        }
        match value {
            Value::List(values) => pending.extend(values.iter()),
            Value::Typed(_, value) => pending.push(value),
            _ => {}
        }
    }
    Ok(count)
}

fn references(value: &Value, entity_out: &mut Vec<u64>, value_out: &mut Vec<u64>) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Reference(id) => entity_out.push(*id),
            Value::ValueReference(id) => value_out.push(*id),
            Value::List(values) => pending.extend(values.iter().rev()),
            Value::Typed(_, value) => pending.push(value),
            _ => {}
        }
    }
}

fn contains_class3_occurrence(value: &Value) -> bool {
    match value {
        Value::ValueReference(_) | Value::ConstantEntity(_) | Value::ConstantValue(_) => true,
        Value::List(values) => values.iter().any(contains_class3_occurrence),
        Value::Typed(_, value) => contains_class3_occurrence(value),
        _ => false,
    }
}

fn contains_resource_value(value: &Value) -> bool {
    match value {
        Value::Resource(_) => true,
        Value::List(values) => values.iter().any(contains_resource_value),
        Value::Typed(_, value) => contains_resource_value(value),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
