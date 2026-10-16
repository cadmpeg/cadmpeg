// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.
//!
//! Recoverable defects omit bounded records and their dependents. A loss
//! reports each omission, and exact source spans remain available for fidelity.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::Arc;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

use self::implementation_level::{DeclaredImplementationLevel, ImplementationLevel};

pub(crate) mod implementation_level;

use crate::lex::{BinaryValue, LexError, Lexer, LiteralAdmission, Token, TokenKind};
use crate::parse::schema_identifier::{
    split_schema_identifier, valid_schema_identifier, AdmittedSchemaIdentifier,
};

mod metadata;
mod parameters;
mod recovery;
pub(crate) mod schema_identifier;

use self::parameters::{ParameterValues, RecordParameters};

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
    /// A metadata literal whose exact bytes remain in its source record.
    UninterpretedLiteral,
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

fn try_clone_value(value: &Value, budget: &DecodeContext<'_>) -> Result<Value, CodecError> {
    let _depth = budget.enter_nested("step_value_copy_depth")?;
    Ok(match value {
        Value::Reference(id) => Value::Reference(*id),
        Value::ExternalReference(id) => Value::ExternalReference(*id),
        Value::ConstantEntity(text) => Value::ConstantEntity(text.to_owned()),
        Value::ExpressValueConstant(text) => Value::ExpressValueConstant(text.to_owned()),
        Value::Integer(value) => Value::Integer(*value),
        Value::Real(value) => Value::Real(*value),
        Value::UninterpretedLiteral => Value::UninterpretedLiteral,
        Value::Enumeration(text) => Value::Enumeration(text.to_owned()),
        Value::String(bytes) => Value::String(bytes.clone()),
        Value::Binary(binary) => Value::Binary(binary.clone()),
        Value::Resource(text) => Value::Resource(text.to_owned()),
        Value::Omitted => Value::Omitted,
        Value::Derived => Value::Derived,
        Value::List(values) => {
            let mut copied = Vec::new();
            for value in values {
                copied.push(try_clone_value(value, budget)?);
            }
            Value::List(copied)
        }
        Value::Typed(name, nested) => {
            Value::Typed(name.to_owned(), Box::new(try_clone_value(nested, budget)?))
        }
    })
}

/// One simple entity leaf within an entity instance.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PartialRecord {
    /// Uppercase standard or `!`-prefixed user-defined entity name.
    pub(crate) name: String,
    /// Explicit external-mapping parameters.
    pub(crate) parameters: RecordParameters,
}

pub(crate) mod partials {
    use super::PartialRecord;

    /// The nonempty partial population of one entity instance.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct RecordPartials(Storage);

    #[derive(Debug, Clone, PartialEq)]
    enum Storage {
        One(PartialRecord),
        Many(Vec<PartialRecord>),
    }

    impl RecordPartials {
        /// Builds the inline population of one leaf without a heap collection.
        pub(crate) fn single(first: PartialRecord) -> Self {
            Self(Storage::One(first))
        }

        pub(super) fn push(self, next: PartialRecord) -> Self {
            let records = match self.0 {
                Storage::One(first) => vec![first, next],
                Storage::Many(mut records) => {
                    records.push(next);
                    records
                }
            };
            Self(Storage::Many(records))
        }

        pub(super) fn shrink_to_fit(&mut self) {
            if let Storage::Many(records) = &mut self.0 {
                records.shrink_to_fit();
            }
        }

        /// The first partial record, which always exists.
        pub(crate) fn first(&self) -> &PartialRecord {
            &self[0]
        }
    }

    impl std::ops::Deref for RecordPartials {
        type Target = [PartialRecord];

        fn deref(&self) -> &Self::Target {
            match &self.0 {
                Storage::One(first) => std::slice::from_ref(first),
                Storage::Many(records) => records,
            }
        }
    }

    impl std::ops::DerefMut for RecordPartials {
        fn deref_mut(&mut self) -> &mut Self::Target {
            match &mut self.0 {
                Storage::One(first) => std::slice::from_mut(first),
                Storage::Many(records) => records,
            }
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
    #[cfg(test)]
    mod tests {
        use super::{PartialRecord, RecordPartials};
        use crate::parse::parameters::RecordParameters;

        #[test]
        fn one_partial_is_inline_and_a_second_leaf_is_appended() {
            let partial = || PartialRecord {
                name: String::from("ITEM"),
                parameters: RecordParameters::default(),
            };
            let parts = RecordPartials::single(partial());
            assert_eq!(parts.len(), 1);
            assert_eq!(parts.first().name, "ITEM");
            let parts = RecordPartials::single(partial()).push(partial());
            assert_eq!(parts.len(), 2);
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

impl RawRecord {
    /// Every parameter of every partial, in source order.
    fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        (&mut self.partials)
            .into_iter()
            .flat_map(|partial| &mut partial.parameters)
    }
}

/// One entity-like record in the HEADER section.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeaderRecord {
    /// Header record name.
    pub(crate) name: String,
    /// Header record parameters.
    pub(crate) parameters: Vec<Value>,
    /// Byte offset of the record name in the source.
    pub(crate) offset: usize,
    /// Byte offset after the terminating semicolon.
    pub(crate) end: usize,
}

/// One DATA section and the source extent of its population.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DataSection {
    /// Edition-3 DATA section parameters.
    pub(crate) parameters: Vec<Value>,
    /// Half-open byte range of the DATA population, excluding ENDSEC.
    pub(crate) span: Range<usize>,
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

impl AnchorEntry {
    /// The bound value followed by each tag value.
    fn values(&self) -> impl Iterator<Item = &Value> {
        std::iter::once(&self.value).chain(self.tags.iter().map(|tag| &tag.value))
    }

    fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        std::iter::once(&mut self.value).chain(self.tags.iter_mut().map(|tag| &mut tag.value))
    }
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
    /// Bounded exchange statements excluded from the interpreted graph.
    omitted_spans: Vec<Range<usize>>,
    schema_identifiers: Vec<AdmittedSchemaIdentifier>,
    implementation_level: DeclaredImplementationLevel,
    entity_ids: EntityIndex,
}

#[derive(Debug, Clone, Default)]
struct EntityIndex(Arc<HashMap<String, Vec<u64>>>);

impl PartialEq for EntityIndex {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl EntityIndex {
    fn build(records: &BTreeMap<u64, RawRecord>) -> EntityIndex {
        let mut index = HashMap::<String, Vec<u64>>::new();
        for (&id, record) in records {
            for partial in &record.partials {
                if let Some(ids) = index.get_mut(partial.name.as_str()) {
                    ids.push(id);
                } else {
                    let name = partial.name.clone();
                    let ids = vec![id];
                    index.insert(name, ids);
                }
            }
        }
        Self(Arc::new(index))
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

    pub(crate) fn omitted_spans(&self) -> &[Range<usize>] {
        &self.omitted_spans
    }

    /// Header-admitted `FILE_SCHEMA` identifiers in source order.
    pub(crate) fn schema_identifiers(&self) -> impl Iterator<Item = &str> {
        self.schema_identifiers
            .iter()
            .map(AdmittedSchemaIdentifier::text)
    }

    pub(crate) fn joined_schema_identifiers(&self) -> String {
        self.schema_identifiers().collect::<Vec<_>>().join(",")
    }

    /// Numeric object-identifier components for the primary schema identifier.
    pub(crate) fn primary_schema_object_identifier(&self) -> Option<Vec<u64>> {
        self.schema_identifiers
            .first()
            .and_then(AdmittedSchemaIdentifier::numeric_object_identifier)
    }

    /// Verbatim `FILE_DESCRIPTION` implementation-level declaration.
    pub(crate) fn implementation_level(&self) -> &str {
        self.implementation_level.text()
    }

    pub(crate) fn decode_string(
        &self,
        bytes: &[u8],
    ) -> Result<String, crate::strings::StringError> {
        crate::strings::decode(bytes, self.implementation_level.level())
    }

    /// Release the source graph and transfer its signature extents for retention.
    pub(crate) fn release_source_graph(&mut self) -> Vec<Range<usize>> {
        self.header.clear();
        self.anchors.clear();
        self.references.clear();
        self.data.clear();
        self.records.clear();
        self.omitted_spans.clear();
        self.schema_identifiers.clear();
        self.entity_ids = EntityIndex::default();
        std::mem::take(&mut self.signatures)
    }

    // The record graph is immutable while its name index exists.
    fn entity_ids(&self) -> &HashMap<String, Vec<u64>> {
        &self.entity_ids.0
    }

    pub(crate) fn has_entity(&self, name: &str) -> bool {
        self.entity_ids().contains_key(name)
    }

    pub(crate) fn has_entity_matching(&self, matches: impl Fn(&str) -> bool) -> bool {
        self.entity_ids().keys().any(|name| matches(name))
    }

    pub(crate) fn matching_entity_ids<'a>(
        &'a self,
        matches: impl Fn(&str) -> bool + 'a,
    ) -> impl Iterator<Item = u64> + 'a {
        self.records.iter().filter_map(move |(&id, record)| {
            record
                .partials
                .iter()
                .any(|partial| matches(&partial.name))
                .then_some(id)
        })
    }

    pub(crate) fn entities(&self, name: &str) -> impl Iterator<Item = (u64, &RawRecord)> {
        self.entity_ids()
            .get(name)
            .into_iter()
            .flatten()
            .map(|id| (*id, &self.records[id]))
    }

    pub(crate) fn entities_any<'a>(
        &'a self,
        names: &'a [&str],
    ) -> impl Iterator<Item = (u64, &'a RawRecord)> + 'a {
        self.records.iter().filter_map(move |(&id, record)| {
            record
                .partials
                .iter()
                .any(|partial| names.contains(&partial.name.as_str()))
                .then_some((id, record))
        })
    }
}

/// Structural or lexical exchange failure.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ParseError {
    /// Tokenization failed.
    #[error(transparent)]
    Lex(#[from] LexError),
    /// The caller's decode session refused the parse.
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
pub(crate) enum ParseDiagnosticKind {
    /// A bounded instance or a dependent instance remains source-only.
    RecordOmitted,
    /// A bounded noncanonical prefix remains source-only.
    PreambleOmitted,
    /// Draft exchange records use the Part 21 instance grammar.
    DraftExchangeGrammar,
    /// EOF closes readable records without a complete exchange terminator.
    EnvelopeIncomplete,
    /// Descriptive header metadata is nonconforming but readable.
    HeaderMetadataNoncanonical,
    /// A bounded presentation value remains source-only.
    PresentationMetadataUnusable,
    /// Complex-entity partials are not in their canonical alphabetical order.
    ComplexPartialsNotAlphabetical,
    /// A simple named carrier omits its inherited `name` value.
    OmittedEntityName,
    /// A known name slot has a present non-string value.
    EntityNameUnreadable,
    /// A `FILE_SCHEMA` object identifier has a component outside the range
    /// that its position permits.
    SchemaObjectIdentifierOutOfRange,
    /// `FILE_DESCRIPTION` declares an implementation level whose grammar is
    /// not implemented; parsing continued with the edition-3 class-3 grammar.
    ImplementationLevelUnverified,
    /// An anchor or REFERENCE expansion passed its value-node limit; the
    /// binding or instance remains source-only.
    ExpansionBounded,
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

/// Parse one exchange structure in the caller's decode session.
pub(crate) fn parse(
    input: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<(Exchange, Vec<ParseDiagnostic>), CodecError> {
    parse_inner(input, ctx).map_err(ParseError::into_codec_error)
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
        diagnostics: Vec::new(),
        omitted_entity_names: None,
        defer_lexical_errors: false,
        budget,
    };
    parser.current = parser.lex_next()?;
    parser.exchange()
}

impl ParseError {
    fn into_codec_error(self) -> CodecError {
        match self {
            Self::Resource(error) => error,
            Self::Lex(error) => error.into_codec_error(),
            error @ Self::Syntax { .. } => CodecError::Malformed(error.to_string()),
        }
    }
}

struct Parser<'input, 'ctx, 'arena> {
    lexer: Lexer<'input, 'ctx, 'arena>,
    current: Option<Token>,
    last_end: usize,
    diagnostics: Vec<ParseDiagnostic>,
    omitted_entity_names: Option<(usize, NonZeroUsize)>,
    defer_lexical_errors: bool,
    budget: &'ctx DecodeContext<'arena>,
}

struct PartialNameList<'a>(&'a partials::RecordPartials);

impl fmt::Display for PartialNameList<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, part) in self.0.iter().enumerate() {
            if index != 0 {
                output.write_str(", ")?;
            }
            output.write_str(&part.name)?;
        }
        Ok(())
    }
}

struct SortedPartialNameList<'a>(&'a [&'a str]);

impl fmt::Display for SortedPartialNameList<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, name) in self.0.iter().enumerate() {
            if index != 0 {
                output.write_str(", ")?;
            }
            output.write_str(name)?;
        }
        Ok(())
    }
}

struct HeaderAdmission {
    implementation_level: DeclaredImplementationLevel,
    schema_identifiers: Vec<AdmittedSchemaIdentifier>,
}

/// External-mapping parameter counts for owned simple named carriers.
///
/// The list is limited to carriers handled by the STEP reader. Context,
/// representation-map, relationship, and shape-definition entities have
/// different first attributes and must keep their positional layout.
/// A present malformed name keeps its slot; only a shorter mapping can omit it.
fn named_carrier_arities(name: &str) -> &'static [usize] {
    match name {
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D" => &[5, 7],
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D" => &[6, 8],
        "LOOP" | "REPRESENTATION_ITEM" | "VERTEX" => &[1],
        "SHELL"
        | "SHELL_BASED_WIREFRAME_MODEL"
        | "VERTEX_SHELL"
        | "WIRE_SHELL"
        | "CARTESIAN_POINT"
        | "CLOSED_SHELL"
        | "CONNECTED_EDGE_SET"
        | "CONNECTED_FACE_SET"
        | "DIRECTION"
        | "DRAUGHTING_CALLOUT"
        | "EDGE_BASED_WIREFRAME_MODEL"
        | "EDGE_LOOP"
        | "FACE_BASED_SURFACE_MODEL"
        | "FACETED_BREP"
        | "GEOMETRIC_CURVE_SET"
        | "GEOMETRIC_SET"
        | "OPEN_SHELL"
        | "MANIFOLD_SOLID_BREP"
        | "PLANE"
        | "POLYLINE"
        | "POLY_LOOP"
        | "REPOSITIONED_TESSELLATED_ITEM"
        | "SHELL_BASED_SURFACE_MODEL"
        | "TESSELLATED_GEOMETRIC_SET"
        | "VERTEX_LOOP"
        | "VERTEX_POINT" => &[2],
        "APLL_POINT"
        | "ADVANCED_BREP_REPRESENTATION"
        | "CONNECTED_EDGE_SUB_SET"
        | "ADVANCED_BREP_SHAPE_REPRESENTATION"
        | "ANNOTATION_OCCURRENCE"
        | "AXIS1_PLACEMENT"
        | "AXIS2_PLACEMENT_2D"
        | "BOUNDARY_CURVE"
        | "BREP_WITH_VOIDS"
        | "CIRCLE"
        | "COMPOSITE_CURVE"
        | "CONNECTED_FACE_SUB_SET"
        | "CURVE_REPLICA"
        | "CYLINDRICAL_SURFACE"
        | "DEFINITIONAL_REPRESENTATION"
        | "DRAUGHTING_MODEL"
        | "EDGE"
        | "FACE_BOUND"
        | "FACE_OUTER_BOUND"
        | "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION"
        | "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION"
        | "LINE"
        | "MANIFOLD_SURFACE_SHAPE_REPRESENTATION"
        | "MAPPED_ITEM"
        | "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"
        | "MEASURE_REPRESENTATION_ITEM"
        | "OUTER_BOUNDARY_CURVE"
        | "PARABOLA"
        | "PCURVE"
        | "REPRESENTATION"
        | "SHAPE_DIMENSION_REPRESENTATION"
        | "SHAPE_REPRESENTATION"
        | "SHAPE_REPRESENTATION_WITH_PARAMETERS"
        | "SPHERICAL_SURFACE"
        | "SUBFACE"
        | "SURFACE_OF_LINEAR_EXTRUSION"
        | "SURFACE_OF_REVOLUTION"
        | "SURFACE_REPLICA"
        | "TESSELLATED_CURVE_SET"
        | "TESSELLATED_SHAPE_REPRESENTATION"
        | "TESSELLATED_SHELL"
        | "TESSELLATED_SOLID"
        | "VECTOR" => &[3],
        "APLL_POINT_WITH_SURFACE"
        | "OFFSET_CURVE_2D"
        | "ADVANCED_FACE"
        | "ANNOTATION_FILL_AREA_OCCURRENCE"
        | "ANNOTATION_PLANE"
        | "AXIS2_PLACEMENT_3D"
        | "CONICAL_SURFACE"
        | "CURVE_BOUNDED_SURFACE"
        | "ELLIPSE"
        | "FACE_SURFACE"
        | "HYPERBOLA"
        | "INTERSECTION_CURVE"
        | "OFFSET_SURFACE"
        | "ORIENTED_CLOSED_SHELL"
        | "ORIENTED_FACE"
        | "ORIENTED_OPEN_SHELL"
        | "SEAM_CURVE"
        | "SUBEDGE"
        | "SURFACE_CURVE"
        | "TOROIDAL_SURFACE" => &[4],
        "TESSELLATED_FACE"
        | "DEGENERATE_TOROIDAL_SURFACE"
        | "EDGE_CURVE"
        | "OFFSET_CURVE_3D"
        | "ORIENTED_EDGE" => &[5],
        "BEZIER_CURVE"
        | "QUASI_UNIFORM_CURVE"
        | "UNIFORM_CURVE"
        | "SEAM_EDGE"
        | "TRIMMED_CURVE" => &[6],
        "RECTANGULAR_TRIMMED_SURFACE" => &[8],
        "B_SPLINE_CURVE_WITH_KNOTS" => &[9],
        "B_SPLINE_SURFACE_WITH_KNOTS" => &[13],
        _ => &[],
    }
}

fn omitted_entity_name(partial: &PartialRecord) -> bool {
    named_carrier_arities(&partial.name).contains(&(partial.parameters.len() + 1))
        && !matches!(
            partial.parameters.first(),
            Some(Value::String(_) | Value::Omitted)
        )
}

impl Parser<'_, '_, '_> {
    fn exchange(mut self) -> Result<(Exchange, Vec<ParseDiagnostic>), ParseError> {
        let draft = self.lexer.is_draft();
        let mut omitted_spans = Vec::new();
        if let Some(offset) =
            crate::codec::draft_exchange_offset(self.lexer.input()).filter(|&offset| offset != 0)
        {
            self.diagnostic(
                0,
                ParseDiagnosticKind::PreambleOmitted,
                format_args!(
                    "bounded prefix before the draft exchange omitted; exact source retained"
                ),
            );
            omitted_spans.push(0..offset);
        }
        if draft {
            self.diagnostic(0, ParseDiagnosticKind::DraftExchangeGrammar, format_args!("pre-standard STEP/FILE_IDENTIFICATION exchange; DATA parsed with the Part 21 instance grammar, with draft @ assignments and !* comments"));
        }
        self.name(if draft { "STEP" } else { "ISO-10303-21" })?;
        self.punct(&TokenKind::Semicolon)?;
        self.name("HEADER")?;
        self.defer_lexical_errors = true;
        self.lexer.set_literal_admission(LiteralAdmission::Metadata);
        self.punct(&TokenKind::Semicolon)?;
        let mut header = Vec::new();
        while !self.peek_name("ENDSEC") && self.current.is_some() {
            if let Some(record) = self.recover_header_record(&mut omitted_spans)? {
                header.push(record);
            }
        }
        self.defer_lexical_errors = false;
        self.lexer.set_literal_admission(LiteralAdmission::Required);
        self.name("ENDSEC")?;
        self.punct(&TokenKind::Semicolon)?;
        let admission = if draft && !header.iter().any(|record| record.name == "FILE_SCHEMA") {
            Ok((
                HeaderAdmission {
                    implementation_level: DeclaredImplementationLevel::new(String::new()),
                    schema_identifiers: Vec::new(),
                },
                Vec::new(),
            ))
        } else {
            validate_header(&header)
        };
        let (header_admission, header_diagnostics) = match admission {
            Ok(admitted) => admitted,
            Err(ValidationError::Invalid(message)) => return self.err(message),
            Err(ValidationError::Resource(error)) => return Err(ParseError::Resource(error)),
        };
        let implementation_level = header_admission.implementation_level.level();

        self.diagnostics.extend(header_diagnostics);
        let schema_names_for_matching =
            schema_names_for_matching(&header_admission.schema_identifiers);
        let header_data_references = match validate_header_sections(
            implementation_level,
            &header,
            &schema_names_for_matching,
        ) {
            Ok(references) => references,
            Err(ValidationError::Invalid(message)) => {
                push_header_metadata_diagnostic(
                    &mut self.diagnostics,
                    header.first().map_or(0, |record| record.offset),
                    format_args!("{message}"),
                );
                Vec::new()
            }
            Err(ValidationError::Resource(error)) => return Err(ParseError::Resource(error)),
        };
        let mut anchors = Vec::new();
        let mut anchor_spans = Vec::new();
        if let Some(level) = implementation_level.edition3_sections_forbidden_by() {
            if self.peek_name("ANCHOR") || self.peek_name("REFERENCE") {
                self.diagnostic(self.current_offset(), ParseDiagnosticKind::ImplementationLevelUnverified, format_args!("{level} forbids ANCHOR and REFERENCE sections; readable sections parsed with the 4;3 section grammar"));
            }
        }
        if self.peek_name("ANCHOR") {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
                let start = self.current_offset();
                let TokenKind::Resource(name) = self.next_kind()? else {
                    return self.err("expected anchor name");
                };
                if !valid_anchor_name(&name) {
                    return self.err("anchor name must contain a non-digit character");
                }
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
                    self.punct(&TokenKind::Colon)?;
                    let value = self.value()?;
                    if !is_anchor_item(&value) {
                        return self.err("invalid anchor tag item");
                    }
                    self.punct(&TokenKind::RBrace)?;

                    tags.push(AnchorTag { name, value });
                }
                tags.shrink_to_fit();

                self.punct(&TokenKind::Semicolon)?;
                anchors.push(AnchorEntry { name, value, tags });
                anchor_spans.push(start..self.previous_end());
            }
            self.next_kind()?;
            self.lexer.set_allow_print_controls(true);
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut reference_entries = Vec::new();

        let mut external_reference_ids = BTreeSet::new();
        let mut external_value_reference_ids = BTreeSet::new();
        if self.peek_name("REFERENCE") {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC") {
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
                if same_kind.contains(&id) {
                    return self.err("duplicate reference name");
                }
                same_kind.insert(id);
                if other_kind.contains(&id) {
                    return self.err("duplicate external occurrence integer");
                }
                self.punct(&TokenKind::Equals)?;
                let TokenKind::Resource(uri) = self.next_kind()? else {
                    return self.err("expected reference URI");
                };
                self.punct(&TokenKind::Semicolon)?;
                reference_entries.push(ReferenceEntry { name, uri });
            }
            self.next_kind()?;
            self.lexer.set_allow_print_controls(true);
            self.punct(&TokenKind::Semicolon)?;
        }
        let mut data: Vec<DataSection> = Vec::new();
        let mut records = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();

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
                let parameters = self.parameter_nesting(Self::parameters_inner)?;
                if let Err(message) = valid_data_parameters(
                    &parameters,
                    &schema_names_for_matching,
                    implementation_level,
                    &mut data_section_names,
                ) {
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
            self.defer_lexical_errors = true;
            self.punct(&TokenKind::Semicolon)?;
            let record_start = self.current_offset();
            while !self.peek_name("ENDSEC") && self.current.is_some() {
                self.recover_data_record(&mut records, &mut ambiguous, &mut omitted_spans)?;
            }
            let record_end = self.current_offset();
            self.defer_lexical_errors = false;
            if self.current.is_some() {
                self.name("ENDSEC")?;
                if self.current.is_some() {
                    self.punct(&TokenKind::Semicolon)?;
                }
            }

            data.push(DataSection {
                parameters,
                span: record_start..record_end,
            });
        }
        if !implementation_level.is_edition3() && data.is_empty() {
            return self.err("historical implementation levels require one DATA section");
        }
        if !header_admission.implementation_level.is_unverified()
            && implementation_level.is_edition3()
            && data.len() == 1
            && data[0].parameters.is_empty()
            && schema_names_for_matching.len() != 1
        {
            return self.err("an unnamed DATA section requires one FILE_SCHEMA identifier");
        }
        if let Err(message) =
            validate_header_data_references(&header_data_references, &data_section_names)
        {
            push_header_metadata_diagnostic(
                &mut self.diagnostics,
                header.first().map_or(0, |record| record.offset),
                format_args!("{message}"),
            );
        }
        if self.current.is_none() && !data.is_empty() {
            self.diagnostic(self.current_offset(), ParseDiagnosticKind::EnvelopeIncomplete, format_args!("readable DATA records precede EOF; closing exchange envelope is incomplete; no terminator bytes were supplied"));
        } else {
            self.name(if draft { "ENDSTEP" } else { "END-ISO-10303-21" })?;
            if self.current.is_some() {
                self.punct(&TokenKind::Semicolon)?;
            } else {
                self.diagnostic(self.current_offset(), ParseDiagnosticKind::EnvelopeIncomplete, format_args!("exchange terminator precedes EOF without its closing semicolon; no terminator bytes were supplied"));
            }
        }
        let mut signatures = Vec::new();
        if let Some(level) = implementation_level.edition3_sections_forbidden_by() {
            if self.peek_name("SIGNATURE") {
                return self.err(&format!("{level} forbids SIGNATURE sections"));
            }
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
            crate::signature::decode_payload(self.lexer.input(), &payload, self.budget)?;
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
        let mut omissions = ExpansionOmissions::default();
        if !anchors.is_empty() {
            let mut anchor_bindings = BTreeMap::new();
            for anchor in &anchors {
                let value = try_clone_value(&anchor.value, self.budget)?;
                anchor_bindings.insert(anchor.name.clone(), value);
            }
            if anchor_bindings.len() != anchors.len() {
                return self.err("duplicate anchor name");
            }
            let mut bounded = BoundedExpansions::default();
            let mut resolver = AnchorResolver::new(&anchor_bindings, self.budget);
            for anchor in &mut anchors {
                if let Some(bound) = resolver
                    .resolve_item(anchor.values_mut())
                    .map_err(|error| error.into_parse_error(0))?
                {
                    bounded.anchors.insert(anchor.name.clone(), bound);
                }
            }
            for (&id, record) in &mut records {
                let offset = record.span.start;
                if let Some(bound) = resolver
                    .resolve_item(record.values_mut())
                    .map_err(|error| error.into_parse_error(offset))?
                {
                    bounded.records.insert(id, bound);
                }
            }
            self.omit_bounded_expansions(
                bounded,
                &mut anchors,
                &mut anchor_spans,
                &mut records,
                &mut omitted_spans,
                &mut omissions,
            );
        }
        // Validate the source occurrence class before local REFERENCES can
        // replace a forbidden token with an ordinary value.
        let class3_restriction =
            implementation_level
                .class3_occurrence_restriction()
                .filter(|_| {
                    header
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
                        })
                });
        let bounded = resolve_local_references(
            &mut anchors,
            &mut records,
            &reference_entries,
            &omissions.anchors,
            self.budget,
        )?;
        self.omit_bounded_expansions(
            bounded,
            &mut anchors,
            &mut anchor_spans,
            &mut records,
            &mut omitted_spans,
            &mut omissions,
        );
        for (&id, record) in &mut records {
            if record.partials.len() == 1 && omitted_entity_name(&record.partials[0]) {
                let parameters = &mut record.partials[0].parameters;

                parameters.prepend(Value::String(Vec::new()));
                match &mut self.omitted_entity_names {
                    Some((_, count)) => {
                        let Some(next) = count.checked_add(1) else {
                            return Self::err_at(
                                record.span.start,
                                "omitted entity name count exceeds the address space",
                            );
                        };
                        *count = next;
                    }
                    None => {
                        self.omitted_entity_names = Some((record.span.start, NonZeroUsize::MIN));
                    }
                }
            }
            for partial in &record.partials {
                if (record.partials.len() == 1 || partial.name == "REPRESENTATION_ITEM")
                    && named_carrier_arities(&partial.name).contains(&partial.parameters.len())
                    && partial.parameters.first().is_some_and(|value| {
                        !matches!(value, Value::String(_) | Value::Omitted | Value::Derived)
                    })
                {
                    self.diagnostics.push(ParseDiagnostic {
                        offset: record.span.start,
                        kind: ParseDiagnosticKind::EntityNameUnreadable,
                        message: format!("{} #{id} has a non-string name; parameter positions and exact source retained", partial.name),
                    });
                }
            }
        }

        let mut refs = Vec::new();
        let mut value_refs = Vec::new();
        let mut dependent = BoundedExpansions::default();
        for anchor in &anchors {
            let mut bounded_reference = None;
            for value in anchor.values() {
                refs.clear();
                value_refs.clear();
                references(value, &mut refs, &mut value_refs, self.budget)?;
                bounded_reference = refs
                    .iter()
                    .copied()
                    .find(|id| omissions.records.contains(id));
                if bounded_reference.is_some() {
                    break;
                }
            }
            if let Some(id) = bounded_reference {
                dependent.anchors.insert(
                    anchor.name.clone(),
                    format!("depends on instance #{id}, omitted at its expansion limit"),
                );
                continue;
            }
            refs.clear();
            value_refs.clear();
            references(&anchor.value, &mut refs, &mut value_refs, self.budget)?;
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
                references(&tag.value, &mut refs, &mut value_refs, self.budget)?;
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
        self.omit_bounded_expansions(
            dependent,
            &mut anchors,
            &mut anchor_spans,
            &mut records,
            &mut omitted_spans,
            &mut omissions,
        );
        self.omit_unresolved_records(
            &mut records,
            &external_reference_ids,
            &external_value_reference_ids,
            &mut omitted_spans,
        )?;
        if let Some(message) = class3_restriction {
            return self.err(message);
        }
        let has_resource_value = records.values().any(|record| {
            record.partials.iter().any(|partial| {
                partial.parameters.iter().enumerate().any(|(index, value)| {
                    let name_slot = index == 0
                        && (record.partials.len() == 1 || partial.name == "REPRESENTATION_ITEM")
                        && named_carrier_arities(&partial.name).contains(&partial.parameters.len());
                    !name_slot && contains_resource_value(value)
                })
            })
        });
        if has_resource_value {
            return self.err("resource values are only valid in edition-3 anchor items");
        }
        if let Some((offset, count)) = self.omitted_entity_names {
            self.diagnostics.push(ParseDiagnostic {
                offset,
                kind: ParseDiagnosticKind::OmittedEntityName,
                message: format!(
                    "recovered {count} simple named carrier instance(s) with an omitted leading name attribute by inserting an empty name"
                ),
            });
        }
        header.shrink_to_fit();
        anchors.shrink_to_fit();
        reference_entries.shrink_to_fit();
        data.shrink_to_fit();
        signatures.shrink_to_fit();
        let entity_ids = EntityIndex::build(&records);
        Ok((
            Exchange {
                header,
                anchors,
                references: reference_entries,
                data,
                signatures,
                records,
                omitted_spans,
                schema_identifiers: header_admission.schema_identifiers,
                implementation_level: header_admission.implementation_level,
                entity_ids,
            },
            self.diagnostics,
        ))
    }

    fn record(&mut self) -> Result<(u64, RawRecord), ParseError> {
        let start = self.current_offset();
        let diagnostic_start = self.diagnostics.len();
        let id = match self.next_kind()? {
            TokenKind::Instance(id) => id,
            TokenKind::ValueInstance(id) if self.lexer.is_draft() => id,
            _ => return self.err("expected instance name"),
        };
        self.punct(&TokenKind::Equals)?;
        self.budget.charge_entities(1, "step_parse_record")?;
        let mut partials = if self.peek(&TokenKind::LParen) {
            self.next_kind()?;
            let first = self.partial(false)?;
            let mut parts = partials::RecordPartials::single(first);
            while !self.peek(&TokenKind::RParen) {
                let partial = self.partial(false)?;
                parts = parts.push(partial);
            }
            self.next_kind()?;

            let mut canonical_names = Vec::new();
            for part in &parts {
                canonical_names.push(part.name.as_str());
            }
            canonical_names.sort_unstable_by(Ord::cmp);
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
                let message = format!(
                    "complex partial records are not alphabetical: observed ({}), expected ({})",
                    PartialNameList(&parts),
                    SortedPartialNameList(&canonical_names),
                );
                self.diagnostics.push(ParseDiagnostic {
                    offset: start,
                    kind: ParseDiagnosticKind::ComplexPartialsNotAlphabetical,
                    message,
                });
            }
            parts
        } else {
            let first = self.partial(true)?;
            partials::RecordPartials::single(first)
        };
        for diagnostic in &mut self.diagnostics[diagnostic_start..] {
            if diagnostic.kind == ParseDiagnosticKind::PresentationMetadataUnusable {
                diagnostic.offset = start;
            }
        }
        partials.shrink_to_fit();
        self.punct(&TokenKind::Semicolon)?;
        Ok((
            id,
            RawRecord {
                partials,
                span: start..self.previous_end(),
            },
        ))
    }

    fn partial(&mut self, simple: bool) -> Result<PartialRecord, ParseError> {
        let offset = self.current_offset();
        let name = self.take_name()?;
        let presentation = metadata::presentation_record(&name);
        if presentation {
            self.lexer.set_literal_admission(LiteralAdmission::Metadata);
        }
        let first_is_name =
            (simple || name == "REPRESENTATION_ITEM") && !named_carrier_arities(&name).is_empty();
        let parameters = self.parameter_nesting(|parser| {
            parser.parameters_with_name::<RecordParameters>(first_is_name && !presentation)
        })?;
        if first_is_name
            && matches!(parameters.first(), Some(Value::UninterpretedLiteral))
            && !named_carrier_arities(&name).contains(&parameters.len())
        {
            return self.err("unreadable literal in a required DATA attribute");
        }
        self.lexer.set_literal_admission(LiteralAdmission::Required);
        if presentation && metadata::unreadable_literal(&parameters, self.budget)? {
            self.diagnostics.push(ParseDiagnostic {
                offset, kind: ParseDiagnosticKind::PresentationMetadataUnusable,
                message: format!("{name} contains an unusable bounded presentation literal; exact record retained"),
            });
        }
        Ok(PartialRecord { name, parameters })
    }

    fn parameter_nesting<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let budget = self.budget;
        let _nested = budget.enter_nested("step_parse_parameter_nesting")?;
        parse(self)
    }

    fn parameters_inner(&mut self) -> Result<Vec<Value>, ParseError> {
        self.parameters_with_name(false)
    }

    fn parameters_with_name<T: ParameterValues>(
        &mut self,
        first_is_name: bool,
    ) -> Result<T, ParseError> {
        if first_is_name {
            self.lexer.set_literal_admission(LiteralAdmission::Metadata);
        }
        self.punct(&TokenKind::LParen)?;
        let mut values = T::default();
        if self.peek(&TokenKind::RParen) {
            if first_is_name {
                self.lexer.set_literal_admission(LiteralAdmission::Required);
            }
            self.next_kind()?;
            return Ok(values);
        }
        loop {
            if first_is_name
                && values.is_empty()
                && matches!(
                    self.current.as_ref().map(|token| &token.kind),
                    Some(TokenKind::LParen | TokenKind::Name(_) | TokenKind::UserName(_))
                )
            {
                // An omitted name can expose the coordinate list in this slot.
                // Its contents retain required numeric admission.
                self.lexer.set_literal_admission(LiteralAdmission::Required);
            }
            let value = self.value()?;
            if first_is_name {
                self.lexer.set_literal_admission(LiteralAdmission::Required);
            }
            values.push_value(value);
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
                TokenKind::UninterpretedLiteral => Value::UninterpretedLiteral,
                TokenKind::Enumeration(value) => Value::Enumeration(value),
                TokenKind::String(value) => Value::String(value),
                TokenKind::Binary(value) => Value::Binary(value),
                TokenKind::Resource(value) => Value::Resource(value),
                TokenKind::Omitted => Value::Omitted,
                TokenKind::Derived => Value::Derived,
                TokenKind::Name(name) => self.typed_parameter(name)?,
                TokenKind::UserName(name) => self.typed_parameter(format!("!{name}"))?,
                _ => return self.err("expected parameter value"),
            }
        };
        Ok(value)
    }

    fn typed_parameter(&mut self, name: String) -> Result<Value, ParseError> {
        self.parameter_nesting(|parser| {
            let metadata = parser.lexer.allows_uninterpreted_literals();
            parser.punct(&TokenKind::LParen)?;
            if parser.peek(&TokenKind::RParen) {
                if metadata {
                    parser.punct(&TokenKind::RParen)?;
                    return Ok(Value::UninterpretedLiteral);
                }
                return parser.err("typed parameter requires one value");
            }
            let value = parser.value()?;
            if parser.peek(&TokenKind::Comma) {
                if !metadata {
                    return parser.err("typed parameter requires one value");
                }
                while parser.peek(&TokenKind::Comma) {
                    parser.next_kind()?;
                    // discarded-value: consume balanced metadata values; the raw record owns their exact bytes.
                    let _ = parser.value()?;
                }
                parser.punct(&TokenKind::RParen)?;
                return Ok(Value::UninterpretedLiteral);
            }
            parser.punct(&TokenKind::RParen)?;

            Ok(Value::Typed(name, Box::new(value)))
        })
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
            let message = format!("expected {expected}, found {actual}");
            Err(ParseError::Syntax {
                offset: self.current_offset(),
                message,
            })
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
        if let TokenKind::InvalidSource(message) = token.kind {
            return Err(ParseError::Syntax {
                offset: token.span.start,
                message,
            });
        }
        self.last_end = token.span.end;
        self.current = self.lex_next()?;
        Ok(token.kind)
    }
    fn lex_next(&mut self) -> Result<Option<Token>, ParseError> {
        let token = match self.lexer.next_token() {
            Ok(token) => token,
            Err(error) if self.defer_lexical_errors => {
                let offset = error.offset();
                let message = error.to_string();
                if let Some(resource) = error.into_resource_error() {
                    return Err(ParseError::Resource(resource));
                }
                Some(Token {
                    kind: TokenKind::InvalidSource(message),
                    span: offset..offset,
                })
            }
            Err(error) => return Err(ParseError::Lex(error)),
        };
        Ok(token)
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

/// Validate the three required header records, and admit the `FILE_SCHEMA`
/// identifier list.
enum ValidationError {
    Invalid(&'static str),
    Resource(CodecError),
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

fn push_header_metadata_diagnostic(
    diagnostics: &mut Vec<ParseDiagnostic>,
    offset: usize,
    message: std::fmt::Arguments<'_>,
) {
    diagnostics.push(ParseDiagnostic {
        offset,
        kind: ParseDiagnosticKind::HeaderMetadataNoncanonical,
        message: message.to_string(),
    });
}

/// Admit interpretation fields independently of header metadata conformance.
fn validate_header(
    header: &[HeaderRecord],
) -> Result<(HeaderAdmission, Vec<ParseDiagnostic>), ValidationError> {
    const REQUIRED: [&str; 3] = ["FILE_DESCRIPTION", "FILE_NAME", "FILE_SCHEMA"];
    let mut diagnostics = Vec::new();
    let offset = header.first().map_or(0, |record| record.offset);
    let mut schemas = header.iter().filter(|record| record.name == "FILE_SCHEMA");
    let Some(schema_record) = schemas.next() else {
        return invalid("HEADER has no FILE_SCHEMA");
    };
    if schemas.next().is_some() {
        return invalid("HEADER contains duplicate FILE_SCHEMA");
    }
    if header.len() < REQUIRED.len()
        || header
            .iter()
            .zip(REQUIRED)
            .any(|(record, expected)| record.name != expected)
    {
        push_header_metadata_diagnostic(&mut diagnostics, offset, format_args!("HEADER does not begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA; records read by name"));
    }
    let mut descriptions = header
        .iter()
        .filter(|record| record.name == "FILE_DESCRIPTION");
    let description = descriptions.next();
    let text = match (description, descriptions.next()) {
        (Some(record), None) => match record.parameters.get(1) {
            Some(Value::String(bytes)) => {
                decoded_bytes(bytes, ImplementationLevel::LegacyEdition1).unwrap_or_default()
            }
            _ => String::new(),
        },
        _ => String::new(),
    };
    let declaration = DeclaredImplementationLevel::new(text);
    if declaration.is_unverified() {
        let message = format!("FILE_DESCRIPTION implementation level {:?} has no implemented grammar; parsed with the 4;3 grammar", declaration.text());
        diagnostics.push(ParseDiagnostic {
            offset: description.map_or(offset, |record| record.offset),
            kind: ParseDiagnosticKind::ImplementationLevelUnverified,
            message,
        });
    }
    let implementation_level = declaration.level();
    for name in ["FILE_DESCRIPTION", "FILE_NAME"] {
        let count = header.iter().filter(|record| record.name == name).count();
        if count != 1 {
            push_header_metadata_diagnostic(
                &mut diagnostics,
                offset,
                format_args!("HEADER contains {count} {name} records; expected one"),
            );
        }
    }
    for record in header {
        let result = match record.name.as_str() {
            "FILE_DESCRIPTION" => {
                validate_file_description(&record.parameters, implementation_level)
            }
            "FILE_NAME" => validate_file_name(&record.parameters, implementation_level),
            _ => continue,
        };
        match result {
            Ok(()) => {}
            Err(ValidationError::Invalid(message)) => push_header_metadata_diagnostic(
                &mut diagnostics,
                record.offset,
                format_args!("{message}"),
            ),
            Err(error @ ValidationError::Resource(_)) => return Err(error),
        }
    }
    let schema = &schema_record.parameters;
    let Some(Value::List(identifiers)) = schema.first() else {
        return invalid("FILE_SCHEMA must contain one schema identifier list");
    };
    if schema.len() != 1 || identifiers.is_empty() {
        return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
    }
    let mut admitted = Vec::new();
    let mut normalized_identifiers = BTreeSet::new();
    for value in identifiers {
        let identifier = match value {
            Value::String(bytes) => decoded_bytes(bytes, implementation_level),
            _ => None,
        };
        let Some(identifier) = identifier.and_then(AdmittedSchemaIdentifier::admit) else {
            push_header_metadata_diagnostic(&mut diagnostics, schema_record.offset, format_args!("FILE_SCHEMA contains an unusable identifier; schema selected from the readable identifiers"));
            continue;
        };
        let trimmed = identifier.text().trim();

        let normalized = trimmed.to_ascii_uppercase();
        if !normalized_identifiers.insert(normalized) {
            push_header_metadata_diagnostic(
                &mut diagnostics,
                schema_record.offset,
                format_args!(
                    "FILE_SCHEMA repeats an identifier; duplicate excluded from schema selection"
                ),
            );
            continue;
        }
        admitted.push(identifier);
    }
    if admitted.is_empty() {
        return invalid("FILE_SCHEMA has no usable schema identifiers");
    }
    for diagnostic in schema_object_identifier_diagnostics(&admitted, schema_record.offset) {
        diagnostics.push(diagnostic?);
    }
    Ok((
        HeaderAdmission {
            implementation_level: declaration,
            schema_identifiers: admitted,
        },
        diagnostics,
    ))
}

fn validate_file_description(
    description: &[Value],
    implementation_level: ImplementationLevel,
) -> Result<(), ValidationError> {
    let [description_strings, implementation_level_value @ Value::String(_)] = description else {
        return invalid("FILE_DESCRIPTION has invalid parameters");
    };
    if !is_string_list(Some(description_strings)) {
        return invalid("FILE_DESCRIPTION has invalid parameters");
    }
    if !is_decodable_string_list(Some(description_strings), implementation_level)
        || !is_decodable_string(implementation_level_value, implementation_level)
    {
        return invalid("FILE_DESCRIPTION has invalid string encoding");
    }
    if !string_list_within_limit(Some(description_strings), implementation_level, 256)
        || !string_within_limit(implementation_level_value, implementation_level, 256)
    {
        return invalid("FILE_DESCRIPTION contains a string longer than 256 characters");
    }

    Ok(())
}

fn validate_file_name(
    file_name: &[Value],
    implementation_level: ImplementationLevel,
) -> Result<(), ValidationError> {
    // Producer metadata after the author and organization lists may be unset.
    let [file_name_value, file_name_timestamp, authors, organizations, preprocessor, originating_system, authorization] =
        file_name
    else {
        return invalid("FILE_NAME has invalid parameters");
    };
    if !matches!(file_name_value, Value::String(_))
        || !matches!(file_name_timestamp, Value::String(_))
        || !is_string_list(Some(authors))
        || !is_string_list(Some(organizations))
        || !is_string_or_omitted(Some(preprocessor))
        || !is_string_or_omitted(Some(originating_system))
        || !is_string_or_omitted(Some(authorization))
    {
        return invalid("FILE_NAME has invalid parameters");
    }
    let Some(time_stamp) = decoded_string(file_name_timestamp, implementation_level) else {
        return invalid("FILE_NAME has invalid string encoding");
    };
    if !is_decodable_string(file_name_value, implementation_level)
        || !is_decodable_string_list(Some(authors), implementation_level)
        || !is_decodable_string_list(Some(organizations), implementation_level)
        || !is_decodable_string_or_omitted(preprocessor, implementation_level)
        || !is_decodable_string_or_omitted(originating_system, implementation_level)
        || !is_decodable_string_or_omitted(authorization, implementation_level)
    {
        return invalid("FILE_NAME has invalid string encoding");
    }
    if !string_within_limit(file_name_value, implementation_level, 256)
        || !string_within_limit(file_name_timestamp, implementation_level, 256)
        || !string_list_within_limit(Some(authors), implementation_level, 256)
        || !string_list_within_limit(Some(organizations), implementation_level, 256)
        || !string_or_omitted_within_limit(preprocessor, implementation_level, 256)
        || !string_or_omitted_within_limit(originating_system, implementation_level, 256)
        || !string_or_omitted_within_limit(authorization, implementation_level, 256)
    {
        return invalid("FILE_NAME contains a string longer than 256 characters");
    }
    if !time_stamp.is_empty() && !valid_timestamp_text(&time_stamp) {
        return invalid("FILE_NAME has an invalid timestamp");
    }

    Ok(())
}

/// One diagnostic for each `FILE_SCHEMA` identifier that the header admits
/// under its schema name alone.
fn schema_object_identifier_diagnostics(
    admitted: &[AdmittedSchemaIdentifier],
    offset: usize,
) -> impl Iterator<Item = Result<ParseDiagnostic, CodecError>> + '_ {
    admitted
        .iter()
        .filter_map(move |identifier| identifier.out_of_range().map(|(name, component)| Ok(format!("FILE_SCHEMA identifier {name} has an out-of-range object identifier component {component}; the object identifier is not admitted"))
            .map(|message| ParseDiagnostic {
                offset,
                kind: ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange,
                message,
            })))
}

enum HeaderDataReferences {
    FilePopulation(BTreeSet<String>),
    Section(String),
}

fn validate_header_sections(
    implementation_level: ImplementationLevel,
    header: &[HeaderRecord],
    schema_identifiers: &[String],
) -> Result<Vec<HeaderDataReferences>, ValidationError> {
    let has = |name: &str| header.iter().any(|record| record.name == name);
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("FILE_POPULATION") {
        return invalid("2;1 forbids FILE_POPULATION in HEADER");
    }
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("SECTION_LANGUAGE") {
        return invalid("2;1 forbids SECTION_LANGUAGE in HEADER");
    }
    if implementation_level == ImplementationLevel::LegacyEdition1 && has("SECTION_CONTEXT") {
        return invalid("2;1 forbids SECTION_CONTEXT in HEADER");
    }
    match implementation_level {
        ImplementationLevel::LegacyEdition2 if has("SCHEMA_POPULATION") => {
            return invalid("3;1 forbids SCHEMA_POPULATION in HEADER");
        }
        ImplementationLevel::Edition3Class1 if has("SCHEMA_POPULATION") => {
            return invalid("4;1 forbids SCHEMA_POPULATION in HEADER");
        }
        _ => {}
    }

    let mut references = Vec::new();
    let mut user_defined = false;
    let mut schema_population_seen = false;
    let mut language_sections = BTreeSet::new();
    let mut context_sections = BTreeSet::new();
    for record in header {
        if matches!(
            record.name.as_str(),
            "FILE_DESCRIPTION" | "FILE_NAME" | "FILE_SCHEMA"
        ) {
            continue;
        }
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
                if !valid_schema_population(&record.parameters, implementation_level) {
                    return invalid("SCHEMA_POPULATION has invalid parameters");
                }
            }
            "FILE_POPULATION" => {
                let sections = admit_file_population(
                    &record.parameters,
                    schema_identifiers,
                    implementation_level,
                )
                .map_err(|error| error.with_message("FILE_POPULATION has invalid parameters"))?;
                references.push(HeaderDataReferences::FilePopulation(sections));
            }
            "SECTION_LANGUAGE" => {
                let section = valid_section_language(&record.parameters, implementation_level)
                    .map_err(|error| {
                        error.with_message("SECTION_LANGUAGE has invalid parameters")
                    })?;
                let section_copy = section.as_deref().map(std::borrow::ToOwned::to_owned);
                if !language_sections.insert(section_copy) {
                    return invalid("HEADER contains duplicate SECTION_LANGUAGE section");
                }
                if let Some(section) = section {
                    references.push(HeaderDataReferences::Section(section));
                }
            }
            "SECTION_CONTEXT" => {
                let section = valid_section_context(&record.parameters, implementation_level)
                    .map_err(|error| {
                        error.with_message("SECTION_CONTEXT has invalid parameters")
                    })?;
                let section_copy = section.as_deref().map(std::borrow::ToOwned::to_owned);
                if !context_sections.insert(section_copy) {
                    return invalid("HEADER contains duplicate SECTION_CONTEXT section");
                }
                if let Some(section) = section {
                    references.push(HeaderDataReferences::Section(section));
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
) -> bool {
    let [Value::List(identifications)] = parameters else {
        return false;
    };
    if identifications.is_empty() {
        return false;
    }
    for identification in identifications {
        let Value::List(values) = identification else {
            return false;
        };
        let [Value::String(address), time_stamp, digest] = values.as_slice() else {
            return false;
        };
        if decoded_bytes(address, implementation_level).is_none()
            || !valid_optional_timestamp(time_stamp, implementation_level)
            || !valid_optional_base64(digest, implementation_level)
        {
            return false;
        }
    }
    true
}

fn admit_file_population(
    parameters: &[Value],
    schema_identifiers: &[String],
    implementation_level: ImplementationLevel,
) -> Result<BTreeSet<String>, ValidationError> {
    let [Value::String(schema), Value::String(determination), governed_sections] = parameters
    else {
        return invalid("FILE_POPULATION has invalid parameters");
    };
    let Some(schema) = decoded_bytes(schema, implementation_level) else {
        return invalid("FILE_POPULATION has invalid parameters");
    };
    if !valid_schema_identifier(&schema)
        || decoded_bytes(determination, implementation_level).is_none()
        || !schema_identifier_matches(schema_identifiers, &schema)
    {
        return invalid("FILE_POPULATION has invalid parameters");
    }
    match governed_sections {
        Value::Omitted => Ok(BTreeSet::new()),
        Value::List(sections) if !sections.is_empty() => {
            let mut names = BTreeSet::new();
            for section in sections {
                let Some(section) = decoded_string(section, implementation_level) else {
                    return invalid("FILE_POPULATION has invalid parameters");
                };
                if !names.insert(section) {
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
) -> Result<Option<String>, ValidationError> {
    let [section, language] = parameters else {
        return invalid("SECTION_LANGUAGE has invalid parameters");
    };
    let Some(language) = decoded_string(language, implementation_level) else {
        return invalid("SECTION_LANGUAGE has invalid parameters");
    };
    if language.len() != 3 || !language.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return invalid("SECTION_LANGUAGE has invalid parameters");
    }
    valid_optional_section_name(section, implementation_level)
}

fn valid_section_context(
    parameters: &[Value],
    implementation_level: ImplementationLevel,
) -> Result<Option<String>, ValidationError> {
    let [section, Value::List(contexts)] = parameters else {
        return invalid("SECTION_CONTEXT has invalid parameters");
    };
    if contexts.is_empty() {
        return invalid("SECTION_CONTEXT has invalid parameters");
    }
    for context in contexts {
        if decoded_string(context, implementation_level).is_none() {
            return invalid("SECTION_CONTEXT has invalid parameters");
        }
    }
    valid_optional_section_name(section, implementation_level)
}

fn valid_optional_section_name(
    value: &Value,
    implementation_level: ImplementationLevel,
) -> Result<Option<String>, ValidationError> {
    match value {
        Value::Omitted => Ok(None),
        value => decoded_string(value, implementation_level)
            .map(Some)
            .ok_or(ValidationError::Invalid("invalid section name")),
    }
}

fn is_decodable_string(value: &Value, implementation_level: ImplementationLevel) -> bool {
    decoded_string(value, implementation_level).is_some()
}

fn is_decodable_string_list(
    value: Option<&Value>,
    implementation_level: ImplementationLevel,
) -> bool {
    let Some(Value::List(values)) = value else {
        return false;
    };
    if values.is_empty() {
        return false;
    }
    for value in values {
        if !is_decodable_string(value, implementation_level) {
            return false;
        }
    }
    true
}

fn is_decodable_string_or_omitted(
    value: &Value,
    implementation_level: ImplementationLevel,
) -> bool {
    if matches!(value, Value::Omitted) {
        return true;
    }
    is_decodable_string(value, implementation_level)
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
    let Some(Value::List(values)) = value else {
        return false;
    };
    if values.is_empty() {
        return false;
    }
    for value in values {
        if !string_within_limit(value, implementation_level, limit) {
            return false;
        }
    }
    true
}

fn string_or_omitted_within_limit(
    value: &Value,
    implementation_level: ImplementationLevel,
    limit: usize,
) -> bool {
    if matches!(value, Value::Omitted) {
        return true;
    }
    string_within_limit(value, implementation_level, limit)
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

fn decoded_string(value: &Value, implementation_level: ImplementationLevel) -> Option<String> {
    let Value::String(bytes) = value else {
        return None;
    };
    decoded_bytes(bytes, implementation_level)
}

fn decoded_bytes(bytes: &[u8], implementation_level: ImplementationLevel) -> Option<String> {
    crate::strings::decode(bytes, implementation_level).ok()
}

fn schema_identifier_matches(schema_identifiers: &[String], schema_name: &str) -> bool {
    let trimmed = schema_name.trim();
    let mut schema_name = trimmed.to_owned();
    schema_name.make_ascii_uppercase();
    schema_identifiers.iter().any(|identifier| {
        let identifier = identifier.trim();
        identifier == schema_name
            || split_schema_identifier(identifier).is_some_and(|(name, _)| name == schema_name)
    })
}

fn validate_header_data_references(
    references: &[HeaderDataReferences],
    data_section_names: &BTreeSet<String>,
) -> Result<(), &'static str> {
    for reference in references {
        match reference {
            HeaderDataReferences::FilePopulation(sections) => {
                if sections
                    .iter()
                    .any(|section| !data_section_names.contains(section))
                {
                    return Err("FILE_POPULATION names an unknown DATA section");
                }
            }
            HeaderDataReferences::Section(section) => {
                if !data_section_names.contains(section) {
                    return Err("header section reference names an unknown DATA section");
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
) -> Result<(), ValidationError> {
    let [Value::String(section_name), Value::List(schema)] = parameters else {
        return invalid("DATA section parameters must contain a name and one schema");
    };
    let [Value::String(schema_name)] = schema.as_slice() else {
        return invalid("DATA section parameters must contain a name and one schema");
    };
    let Some(section_name) = decoded_bytes(section_name, implementation_level) else {
        return invalid("DATA section parameters contain an invalid string");
    };
    if !section_names.insert(section_name) {
        return invalid("DATA section names must be unique");
    }
    let Some(schema_name) = decoded_bytes(schema_name, implementation_level) else {
        return invalid("DATA section parameters contain an invalid string");
    };
    if !valid_schema_identifier(&schema_name)
        || !schema_identifier_matches(schema_identifiers, &schema_name)
    {
        return invalid("DATA section schema is not listed in FILE_SCHEMA");
    }
    Ok(())
}

/// The admitted `FILE_SCHEMA` identifiers, for schema-name matching.
fn schema_names_for_matching(
    admitted: &[AdmittedSchemaIdentifier],
) -> std::vec::Vec<std::string::String> {
    let mut names = Vec::new();
    for identifier in admitted {
        let source = identifier.text();
        let mut name = source.to_owned();
        name.make_ascii_uppercase();
        names.push(name);
    }
    names
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
        | Value::ExternalReference(_)
        | Value::ConstantEntity(_)
        | Value::ExpressValueConstant(_)
        | Value::Integer(_)
        | Value::Real(_)
        | Value::Enumeration(_)
        | Value::String(_)
        | Value::Binary(_)
        | Value::Resource(_)
        | Value::Omitted => true,
        Value::List(values) => values.iter().all(is_anchor_item),
        Value::Derived | Value::Typed(_, _) | Value::UninterpretedLiteral => false,
    }
}

#[derive(Debug)]
enum ResolveError {
    Syntax(String),
    /// A value-node limit stopped the expansion; the message names the bound.
    Bounded(String),
    Resource(CodecError),
}

impl ResolveError {
    fn into_parse_error(self, offset: usize) -> ParseError {
        match self {
            Self::Syntax(message) | Self::Bounded(message) => {
                ParseError::Syntax { offset, message }
            }
            Self::Resource(error) => ParseError::Resource(error),
        }
    }
}

/// Anchors and instances whose expansion passed a value-node limit, each with
/// the bound that stopped it.
#[derive(Default)]
struct BoundedExpansions {
    anchors: BTreeMap<String, String>,
    records: BTreeMap<u64, String>,
}

/// Anchors and instances that a value-node limit removed from the
/// interpreted graph.
#[derive(Default)]
struct ExpansionOmissions {
    anchors: BTreeSet<String>,
    records: BTreeSet<u64>,
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

impl From<CodecError> for ResolveError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

/// What an item's expansion is known to do before it copies anything.
enum KnownOutcome {
    /// Every expansion in the item fits the remaining nodes.
    Fits,
    /// The item stops at this bound.
    Bounded(String),
    /// Only a full expansion decides the item.
    Unknown,
}

/// The bounds that one top-level expansion met before. An expansion that
/// starts from an empty reference stack checks the same bounds in the same
/// order at every budget, so a node-limit bound recurs at every smaller
/// budget and any other bound, met before the node limit, recurs at every
/// larger budget.
#[derive(Default)]
struct ExpansionBounds {
    /// The largest budget at which the expansion passed the node limit.
    node_limit_at: Option<usize>,
    /// The smallest budget at which the expansion met another bound, with
    /// that bound.
    other_at: Option<(usize, String)>,
}

impl ExpansionBounds {
    fn record(&mut self, budget: usize, bound: &str, node_limit: &str) {
        if bound == node_limit {
            self.node_limit_at = Some(self.node_limit_at.map_or(budget, |at| at.max(budget)));
        } else if self.other_at.as_ref().is_none_or(|(at, _)| budget < *at) {
            self.other_at = Some((budget, bound.to_owned()));
        }
    }

    fn bound_at(&self, budget: usize, node_limit: &str) -> Option<String> {
        if self.node_limit_at.is_some_and(|at| budget <= at) {
            return Some(node_limit.to_owned());
        }
        self.other_at
            .as_ref()
            .filter(|(at, _)| *at <= budget)
            .map(|(_, bound)| bound.clone())
    }
}

struct AnchorResolver<'a, 'ctx, 'arena> {
    anchors: &'a BTreeMap<String, Value>,
    memo: BTreeMap<&'a str, (Value, usize)>,
    bounds: BTreeMap<&'a str, ExpansionBounds>,
    remaining_nodes: usize,
    /// Value nodes that omitted items may still copy; `None` once spent.
    discard_allowance: Option<usize>,
    /// Value nodes that the current item has copied or counted.
    item_nodes: usize,
    budget: &'ctx DecodeContext<'arena>,
}

impl<'a, 'ctx, 'arena> AnchorResolver<'a, 'ctx, 'arena> {
    /// Value nodes that anchor expansion may copy across the whole exchange.
    const MAX_EXPANDED_NODES: usize = 1_000_000;
    /// Value nodes that items omitted at the node limit may copy across the
    /// whole exchange before their expansion stopped.
    const MAX_DISCARDED_EXPANDED_NODES: usize = 16 * Self::MAX_EXPANDED_NODES;

    fn new(anchors: &'a BTreeMap<String, Value>, budget: &'ctx DecodeContext<'arena>) -> Self {
        Self {
            anchors,
            memo: BTreeMap::new(),
            bounds: BTreeMap::new(),
            remaining_nodes: Self::MAX_EXPANDED_NODES,
            discard_allowance: Some(Self::MAX_DISCARDED_EXPANDED_NODES),
            item_nodes: 0,
            budget,
        }
    }

    /// Resolves every value of one anchor or instance. A value that passes
    /// the node limit leaves the item unresolved, returns the bound, and
    /// gives back the nodes that the item's earlier values used.
    fn resolve_item<'v>(
        &mut self,
        values: impl Iterator<Item = &'v mut Value>,
    ) -> Result<Option<String>, ResolveError> {
        let values = values.collect::<Vec<_>>();
        match self.known_outcome(&values) {
            KnownOutcome::Bounded(bound) => return Ok(Some(bound)),
            KnownOutcome::Unknown if self.discard_allowance.is_none() => {
                return Ok(Some(Self::discard_limit_bound()));
            }
            KnownOutcome::Fits | KnownOutcome::Unknown => {}
        }
        let remaining = self.remaining_nodes;
        self.item_nodes = 0;
        for value in values {
            match self.resolve_root(value) {
                Ok(resolved) => *value = resolved,
                Err(ResolveError::Bounded(bound)) => {
                    self.remaining_nodes = remaining;
                    self.discard_allowance = self
                        .discard_allowance
                        .and_then(|allowance| allowance.checked_sub(self.item_nodes));
                    return Ok(Some(bound));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// Decides the item from earlier expansions of its top-level
    /// references, without copying.
    fn known_outcome(&self, values: &[&mut Value]) -> KnownOutcome {
        let node_limit = Self::node_limit_bound();
        let mut budget = self.remaining_nodes;
        let mut pending = values
            .iter()
            .rev()
            .map(|value| &**value)
            .collect::<Vec<_>>();
        while let Some(value) = pending.pop() {
            match value {
                Value::Resource(name) => {
                    let Some(name) = name
                        .strip_prefix('#')
                        .filter(|name| self.anchors.contains_key(*name))
                    else {
                        continue;
                    };
                    if let Some((_, nodes)) = self.memo.get(name) {
                        let Some(rest) = budget.checked_sub(*nodes) else {
                            return KnownOutcome::Bounded(node_limit);
                        };
                        budget = rest;
                    } else if let Some(bound) = self
                        .bounds
                        .get(name)
                        .and_then(|bounds| bounds.bound_at(budget, &node_limit))
                    {
                        return KnownOutcome::Bounded(bound);
                    } else {
                        return KnownOutcome::Unknown;
                    }
                }
                Value::List(values) => pending.extend(values.iter().rev()),
                Value::Typed(_, value) => pending.push(value),
                _ => {}
            }
        }
        KnownOutcome::Fits
    }

    fn resolve_root(&mut self, value: &Value) -> Result<Value, ResolveError> {
        let mut stack = Vec::new();
        let (value, _, expanded_nodes) = self.resolve(value, &mut stack, self.remaining_nodes)?;
        self.remaining_nodes = self
            .remaining_nodes
            .checked_sub(expanded_nodes)
            .ok_or_else(Self::node_limit_error)?;
        Ok(value)
    }

    fn node_limit_bound() -> String {
        format!(
            "anchor expansion exceeds {} value nodes",
            Self::MAX_EXPANDED_NODES
        )
    }

    fn node_limit_error() -> ResolveError {
        ResolveError::Bounded(Self::node_limit_bound())
    }

    fn discard_limit_bound() -> String {
        format!(
            "anchor expansions omitted at the node limit already copied {} value nodes",
            Self::MAX_DISCARDED_EXPANDED_NODES
        )
    }

    fn count_item_nodes(&mut self, nodes: usize) -> Result<(), ResolveError> {
        self.item_nodes = self
            .item_nodes
            .checked_add(nodes)
            .ok_or_else(Self::node_limit_error)?;
        Ok(())
    }

    fn resolve(
        &mut self,
        value: &Value,
        stack: &mut Vec<&'a str>,
        budget: usize,
    ) -> Result<(Value, usize, usize), ResolveError> {
        let _nested = self.budget.enter_nested("step_anchor_reference")?;
        self.count_item_nodes(1)?;
        if let Value::Resource(name) = value {
            if let Some((name, source)) = name
                .strip_prefix('#')
                .and_then(|name| self.anchors.get_key_value(name))
            {
                let name = name.as_str();
                if let Some((value, nodes)) = self.memo.get(name) {
                    if *nodes > budget {
                        return Err(Self::node_limit_error());
                    }
                    let (value, nodes) = (try_clone_value(value, self.budget)?, *nodes);
                    self.count_item_nodes(nodes)?;
                    return Ok((value, nodes, nodes));
                }
                if stack.contains(&name) {
                    let message = format!("cyclic anchor binding <{name}>");
                    return Err(message.into());
                }
                let top_level = stack.is_empty();
                let expanded = self.expand(name, source, stack, budget);
                if top_level {
                    if let Err(ResolveError::Bounded(bound)) = &expanded {
                        self.bounds.entry(name).or_default().record(
                            budget,
                            bound,
                            &Self::node_limit_bound(),
                        );
                    }
                }
                return expanded;
            }
        }
        match value {
            Value::List(values) => {
                let mut nodes = 1usize;
                let mut expanded_nodes = 0usize;
                let mut resolved = Vec::new();
                for value in values {
                    let remaining = budget
                        .checked_sub(expanded_nodes)
                        .ok_or_else(Self::node_limit_error)?;
                    let (value, child_nodes, child_expanded_nodes) =
                        self.resolve(value, stack, remaining)?;
                    nodes = nodes
                        .checked_add(child_nodes)
                        .ok_or_else(Self::node_limit_error)?;
                    expanded_nodes = expanded_nodes
                        .checked_add(child_expanded_nodes)
                        .ok_or_else(Self::node_limit_error)?;
                    resolved.push(value);
                }
                Ok((Value::List(resolved), nodes, expanded_nodes))
            }
            Value::Typed(name, value) => {
                let (value, nodes, expanded_nodes) = self.resolve(value, stack, budget)?;
                let value = Value::Typed(name.to_owned(), Box::new(value));
                Ok((
                    value,
                    nodes.checked_add(1).ok_or_else(Self::node_limit_error)?,
                    expanded_nodes,
                ))
            }
            value => Ok((try_clone_value(value, self.budget)?, 1, 0)),
        }
    }

    /// Expands anchor `name`, which is not memoized and not on `stack`.
    fn expand(
        &mut self,
        name: &'a str,
        source: &Value,
        stack: &mut Vec<&'a str>,
        budget: usize,
    ) -> Result<(Value, usize, usize), ResolveError> {
        let counted = value_node_count(source, Self::MAX_EXPANDED_NODES, self.budget);
        self.count_item_nodes(
            counted
                .as_ref()
                .map_or(Self::MAX_EXPANDED_NODES, |nodes| *nodes),
        )?;
        counted?;
        stack.push(name);
        let resolved = self.resolve(source, stack, budget);
        stack.pop();
        let (value, nodes, _) = resolved?;
        if nodes > budget {
            return Err(Self::node_limit_error());
        }
        let memo = try_clone_value(&value, self.budget)?;
        self.memo.insert(name, (memo, nodes));
        Ok((value, nodes, nodes))
    }
}

struct ReferenceResolver<'a, 'ctx, 'arena> {
    bindings: BTreeMap<ReferenceName, &'a str>,
    anchors: &'a BTreeMap<String, Value>,
    omitted_anchors: &'a BTreeSet<String>,
    stack: Vec<ReferenceName>,
    /// Value nodes that each top-level occurrence used when it expanded.
    used: BTreeMap<ReferenceName, usize>,
    bounds: BTreeMap<ReferenceName, ExpansionBounds>,
    remaining_nodes: usize,
    /// Value nodes that omitted items may still copy; `None` once spent.
    discard_allowance: Option<usize>,
    /// Value nodes that the current item has copied.
    item_nodes: usize,
    budget: &'ctx DecodeContext<'arena>,
}

impl<'a, 'ctx, 'arena> ReferenceResolver<'a, 'ctx, 'arena> {
    /// Value nodes that REFERENCE expansion may copy across the whole
    /// exchange.
    const MAX_MATERIALIZED_NODES: usize = 1_000_000;
    /// Value nodes that items omitted at the node limit may copy across the
    /// whole exchange before their expansion stopped.
    const MAX_DISCARDED_MATERIALIZED_NODES: usize = 16 * Self::MAX_MATERIALIZED_NODES;

    fn new(
        references: &'a [ReferenceEntry],
        anchors: &'a BTreeMap<String, Value>,
        omitted_anchors: &'a BTreeSet<String>,
        budget: &'ctx DecodeContext<'arena>,
    ) -> Self {
        let bindings = references
            .iter()
            .map(|reference| (reference.name, reference.uri.as_str()))
            .collect();
        Self {
            bindings,
            anchors,
            omitted_anchors,
            stack: Vec::new(),
            used: BTreeMap::new(),
            bounds: BTreeMap::new(),
            remaining_nodes: Self::MAX_MATERIALIZED_NODES,
            discard_allowance: Some(Self::MAX_DISCARDED_MATERIALIZED_NODES),
            item_nodes: 0,
            budget,
        }
    }

    fn node_limit_bound() -> String {
        format!(
            "external reference expansion exceeds {} value nodes",
            Self::MAX_MATERIALIZED_NODES
        )
    }

    fn node_limit_error() -> ResolveError {
        ResolveError::Bounded(Self::node_limit_bound())
    }

    fn omitted_anchor_bound(key: ReferenceName, fragment: &str) -> String {
        format!("REFERENCE {key} names anchor <{fragment}>, omitted at its expansion limit")
    }

    fn discard_limit_bound() -> String {
        format!(
            "external reference expansions omitted at the node limit already copied {} value nodes",
            Self::MAX_DISCARDED_MATERIALIZED_NODES
        )
    }

    /// Resolves every value of one anchor or instance. A value that passes
    /// the node limit leaves the item unresolved, returns the bound, and
    /// gives back the nodes that the item's earlier values used.
    fn resolve_item<'v>(
        &mut self,
        values: impl Iterator<Item = &'v mut Value>,
    ) -> Result<Option<String>, ResolveError> {
        let values = values.collect::<Vec<_>>();
        match self.known_outcome(&values) {
            KnownOutcome::Bounded(bound) => return Ok(Some(bound)),
            KnownOutcome::Unknown if self.discard_allowance.is_none() => {
                return Ok(Some(Self::discard_limit_bound()));
            }
            KnownOutcome::Fits | KnownOutcome::Unknown => {}
        }
        let remaining = self.remaining_nodes;
        self.item_nodes = 0;
        for value in values {
            match self.resolve_value(value) {
                Ok(resolved) => *value = resolved,
                Err(ResolveError::Bounded(bound)) => {
                    self.remaining_nodes = remaining;
                    self.discard_allowance = self
                        .discard_allowance
                        .and_then(|allowance| allowance.checked_sub(self.item_nodes));
                    return Ok(Some(bound));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// Decides the item from earlier expansions of its top-level
    /// occurrences, without copying.
    fn known_outcome(&self, values: &[&mut Value]) -> KnownOutcome {
        let node_limit = Self::node_limit_bound();
        let mut budget = self.remaining_nodes;
        let mut pending = values
            .iter()
            .rev()
            .map(|value| &**value)
            .collect::<Vec<_>>();
        while let Some(value) = pending.pop() {
            let key = match value {
                Value::Reference(id) => ReferenceName::Entity(*id),
                Value::ExternalReference(id) => ReferenceName::Value(*id),
                Value::List(values) => {
                    pending.extend(values.iter().rev());
                    continue;
                }
                Value::Typed(_, value) => {
                    pending.push(value);
                    continue;
                }
                _ => continue,
            };
            let Some(fragment) = self
                .bindings
                .get(&key)
                .and_then(|uri| uri.split_once('#'))
                .and_then(|(path, fragment)| path.is_empty().then_some(fragment))
            else {
                continue;
            };
            if self.omitted_anchors.contains(fragment) {
                return KnownOutcome::Bounded(Self::omitted_anchor_bound(key, fragment));
            }
            if !self.anchors.contains_key(fragment) {
                continue;
            }
            if let Some(used) = self.used.get(&key) {
                let Some(rest) = budget.checked_sub(*used) else {
                    return KnownOutcome::Bounded(node_limit);
                };
                budget = rest;
            } else if let Some(bound) = self
                .bounds
                .get(&key)
                .and_then(|bounds| bounds.bound_at(budget, &node_limit))
            {
                return KnownOutcome::Bounded(bound);
            } else {
                return KnownOutcome::Unknown;
            }
        }
        KnownOutcome::Fits
    }

    fn clone_leaf(&mut self, value: &Value) -> Result<Value, ResolveError> {
        self.consume_materialized_node()?;
        Ok(try_clone_value(value, self.budget)?)
    }

    fn resolve_value(&mut self, value: &Value) -> Result<Value, ResolveError> {
        let _nested = self.budget.enter_nested("step_reference_expansion")?;
        self.item_nodes = self
            .item_nodes
            .checked_add(1)
            .ok_or_else(Self::node_limit_error)?;
        match value {
            Value::Reference(id) => self.resolve_occurrence(ReferenceName::Entity(*id), value),
            Value::ExternalReference(id) => {
                self.resolve_occurrence(ReferenceName::Value(*id), value)
            }
            Value::List(values) => {
                self.consume_materialized_node()?;
                if !self.stack.is_empty() && values.len() > self.remaining_nodes {
                    return Err(Self::node_limit_error());
                }
                let mut resolved = Vec::new();
                for value in values {
                    resolved.push(self.resolve_value(value)?);
                }
                Ok(Value::List(resolved))
            }
            Value::Typed(name, value) => {
                self.consume_materialized_node()?;
                let resolved = self.resolve_value(value)?;
                Ok(Value::Typed(name.to_owned(), Box::new(resolved)))
            }
            _ => self.clone_leaf(value),
        }
    }

    fn resolve_occurrence(
        &mut self,
        key: ReferenceName,
        original: &Value,
    ) -> Result<Value, ResolveError> {
        let Some(uri) = self.bindings.get(&key).copied() else {
            return self.clone_leaf(original);
        };
        let Some((path, fragment)) = uri.split_once('#') else {
            return self.clone_leaf(&Value::Omitted);
        };
        if !path.is_empty() {
            return self.clone_leaf(original);
        }
        if self.stack.contains(&key) {
            return self.clone_leaf(&Value::Omitted);
        }
        if self.omitted_anchors.contains(fragment) {
            return Err(ResolveError::Bounded(Self::omitted_anchor_bound(
                key, fragment,
            )));
        }
        let Some(anchor) = self.anchors.get(fragment) else {
            return if is_uuid_fragment(fragment) {
                self.clone_leaf(original)
            } else {
                self.clone_leaf(&Value::Omitted)
            };
        };
        let top_level = self.stack.is_empty();
        let remaining = self.remaining_nodes;
        self.stack.push(key);
        let resolved = self.resolve_value(anchor);
        self.stack.pop();
        if top_level {
            match &resolved {
                Ok(_) => {
                    if let Some(used) = remaining.checked_sub(self.remaining_nodes) {
                        self.used.insert(key, used);
                    }
                }
                Err(ResolveError::Bounded(bound)) => {
                    self.bounds.entry(key).or_default().record(
                        remaining,
                        bound,
                        &Self::node_limit_bound(),
                    );
                }
                Err(_) => {}
            }
        }
        let resolved = resolved?;
        if let Value::Resource(uri) = &resolved {
            return if uri.contains('#') {
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
            self.remaining_nodes = self
                .remaining_nodes
                .checked_sub(1)
                .ok_or_else(Self::node_limit_error)?;
        }
        Ok(())
    }
}

/// Resolves local REFERENCE occurrences and returns the anchors and instances
/// whose expansion passed the value-node limit.
fn resolve_local_references(
    anchors: &mut [AnchorEntry],
    records: &mut BTreeMap<u64, RawRecord>,
    references: &[ReferenceEntry],
    omitted_anchors: &BTreeSet<String>,
    budget: &DecodeContext<'_>,
) -> Result<BoundedExpansions, ParseError> {
    let mut bounded = BoundedExpansions::default();
    if references.is_empty() {
        return Ok(bounded);
    }

    let mut anchor_bindings = BTreeMap::new();
    for anchor in anchors.iter() {
        let value = try_clone_value(&anchor.value, budget)?;
        anchor_bindings.insert(anchor.name.clone(), value);
    }
    let mut resolver =
        ReferenceResolver::new(references, &anchor_bindings, omitted_anchors, budget);
    for anchor in anchors {
        if let Some(bound) = resolver
            .resolve_item(anchor.values_mut())
            .map_err(|error| error.into_parse_error(0))?
        {
            bounded.anchors.insert(anchor.name.clone(), bound);
        }
    }
    for (&id, record) in records.iter_mut() {
        let offset = record.span.start;
        if let Some(bound) = resolver
            .resolve_item(record.values_mut())
            .map_err(|error| error.into_parse_error(offset))?
        {
            bounded.records.insert(id, bound);
        }
    }
    Ok(bounded)
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
            matches!(index, 8 | 13 | 18 | 23)
                .then_some(*byte == b'-')
                .unwrap_or_else(|| byte.is_ascii_hexdigit())
        })
}

/// Count the value nodes of `value` before anchor expansion copies it. A
/// value larger than `limit` nodes is [`ResolveError::Bounded`].
fn value_node_count(
    value: &Value,
    limit: usize,
    budget: &DecodeContext<'_>,
) -> Result<usize, ResolveError> {
    fn visit(
        value: &Value,
        remaining: &mut usize,
        limit: usize,
        budget: &DecodeContext<'_>,
    ) -> Result<(), ResolveError> {
        let _depth_guard = budget.enter_nested("step_value_node_count")?;
        *remaining = remaining.checked_sub(1).ok_or_else(|| {
            ResolveError::Bounded(format!("anchor value exceeds {limit} value nodes"))
        })?;
        match value {
            Value::List(values) => {
                for child in values {
                    visit(child, remaining, limit, budget)?;
                }
            }
            Value::Typed(_, child) => visit(child, remaining, limit, budget)?,
            _ => {}
        }
        Ok(())
    }

    let mut remaining = limit;
    visit(value, &mut remaining, limit, budget)?;
    Ok(limit - remaining)
}

fn references(
    value: &Value,
    entity_out: &mut Vec<u64>,
    value_out: &mut Vec<u64>,
    budget: &DecodeContext<'_>,
) -> Result<(), ParseError> {
    recovery::visit_references(value, budget, &mut |id, value_instance| {
        if value_instance {
            value_out.push(id);
        } else {
            entity_out.push(id);
        }
        Ok(())
    })
}

fn contains_class3_occurrence(value: &Value) -> bool {
    match value {
        Value::ExternalReference(_) | Value::ConstantEntity(_) | Value::ExpressValueConstant(_) => {
            true
        }
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
