// SPDX-License-Identifier: Apache-2.0
//! Generic Part 21 record-graph parser.
//!
//! The parser accepts only source deviations whose value remains unambiguous:
//! the deviation must be recoverable without guessing, observed in a real
//! producer, represented by its own diagnostic kind, and rejectable by strict
//! decode policy. Ambiguous records and duplicate names remain parse errors.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
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
            budget.charge_work(u64_from_index(text.len()), operation)?;
            Value::ConstantEntity(budget.copy_retained_text(text, operation)?)
        }
        Value::ExpressValueConstant(text) => {
            budget.charge_work(u64_from_index(text.len()), operation)?;
            Value::ExpressValueConstant(budget.copy_retained_text(text, operation)?)
        }
        Value::Integer(value) => Value::Integer(*value),
        Value::Real(value) => Value::Real(*value),
        Value::Enumeration(text) => {
            budget.charge_work(u64_from_index(text.len()), operation)?;
            Value::Enumeration(budget.copy_retained_text(text, operation)?)
        }
        Value::String(bytes) => {
            budget.charge_work(u64_from_index(bytes.len()), operation)?;
            let copied = budget.copy_slice(bytes, operation)?;
            Value::String(copied)
        }
        Value::Binary(binary) => Value::Binary(binary.try_clone_for_decode(budget, operation)?),
        Value::Resource(text) => {
            budget.charge_work(u64_from_index(text.len()), operation)?;
            Value::Resource(budget.copy_retained_text(text, operation)?)
        }
        Value::Omitted => Value::Omitted,
        Value::Derived => Value::Derived,
        Value::List(values) => {
            let mut copied = budget.collection_vec(values.len(), operation)?;
            for value in budget.admit_iter(values.as_slice(), "STEP try clone value value traversal").map_err(cadmpeg_core::CodecError::from)? {
                copied.push(try_clone_value(value, budget, operation)?);
            }
            Value::List(copied)
        }
        Value::Typed(name, nested) => {
            budget.charge_work(u64_from_index(name.len()), operation)?;
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
struct EntityIndex(Arc<HashMap<String, Vec<u64>>>);

impl PartialEq for EntityIndex {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl EntityIndex {
    fn build(
        records: &BTreeMap<u64, RawRecord>,
        budget: &DecodeContext<'_>,
    ) -> Result<Self, ParseError> {
        let mut index = HashMap::<String, Vec<u64>>::new();
        for (&id, record) in budget.admit_iter(records, "STEP build traversal").map_err(cadmpeg_core::CodecError::from)? {
            for partial in budget.admit_iter(&(record.partials)[..], "STEP build traversal").map_err(cadmpeg_core::CodecError::from)? {
                if let Some(ids) = index.get_mut(partial.name.as_str()) {
                    budget.push_vec(ids, id, "step_entity_index_ids")?;
                } else {
                    let name = budget
                        .copy_retained_text(&partial.name, "step_entity_index_name_storage")?;
                    let ids = budget.collect_vec([id], "step_entity_index_ids")?;
                    budget.insert_hash_map(&mut index, name, ids, "step_entity_index_names")?;
                }
            }
        }
        Ok(Self(Arc::new(index)))
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
        let operation = "step_schema_identifier_list";
        let len = ctx.admit_iter(&(self.schema_identifiers)[..], "STEP joined schema identifiers traversal").map_err(cadmpeg_core::CodecError::from)?.enumerate().try_fold(
            0usize,
            |sum, (index, identifier)| {
                sum.checked_add(identifier.text().len())
                    .and_then(|sum| sum.checked_add(usize::from(index != 0)))
            },
        );
        let len = len.ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
        let mut joined = ctx.retained_string(len, operation)?;

        for identifier in self.schema_identifiers() {
            if !joined.is_empty() {
                joined.push(',');
            }
            joined.push_str(identifier);
        }
        Ok(joined)
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
    fn entity_ids(&self) -> &HashMap<String, Vec<u64>> {
        &self.entity_ids.0
    }

    pub(crate) fn has_entity(&self, name: &str) -> bool {
        self.entity_ids().contains_key(name)
    }

    pub(crate) fn has_entity_matching(&self, ctx: &DecodeContext<'_>, matches: impl Fn(&str) -> bool) -> Result<bool, CodecError> {
        Ok(ctx.admit_iter(self.entity_ids(), "STEP entity name index traversal")?.any(|(name, _)| matches(name)))
    }

    pub(crate) fn matching_entity_ids<'a>(
        &'a self,
        ctx: &'a DecodeContext<'a>,
        matches: impl Fn(&str) -> bool + 'a,
    ) -> Result<impl Iterator<Item = Result<u64, CodecError>> + 'a, CodecError> {
        Ok(ctx.admit_iter(&self.records, "STEP matching entity record traversal")?
            .map(move |(&id, record)| -> Result<Option<u64>, CodecError> {
                let matched = ctx.admit_iter(&record.partials[..], "STEP matching entity partial traversal")?
                    .any(|partial| matches(&partial.name));
                Ok(matched.then_some(id))
            }).filter_map(Result::transpose))
    }

    pub(crate) fn entities<'a>(&'a self, ctx: &DecodeContext<'_>, name: &str) -> Result<impl Iterator<Item = (u64, &'a RawRecord)> + 'a, CodecError> {
        let ids = self.entity_ids().get(name).map_or(&[][..], Vec::as_slice);
        Ok(ctx.admit_iter(ids, "STEP indexed entity identifier traversal")?
            .map(|id| (*id, &self.records[id])))
    }

    pub(crate) fn entities_any<'a>(
        &'a self,
        ctx: &'a DecodeContext<'a>,
        names: &'a [&str],
    ) -> Result<impl Iterator<Item = Result<(u64, &'a RawRecord), CodecError>> + 'a, CodecError> {
        Ok(ctx.admit_iter(&self.records, "STEP entity union record traversal")?
            .map(move |(&id, record)| -> Result<Option<(u64, &'a RawRecord)>, CodecError> {
                let matched = ctx.admit_iter(&record.partials[..], "STEP entity union partial traversal")?
                    .any(|partial| names.contains(&partial.name.as_str()));
                Ok(matched.then_some((id, record)))
            }).filter_map(Result::transpose))
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

/// Parse one exchange structure while charging the caller's decode session.
pub(crate) fn parse_with_context(
    input: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<(Exchange, Vec<ParseDiagnostic>), CodecError> {
    parse_inner(input, ctx).or_else(|error| Err(error.into_codec_error(ctx)?))
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
            Self::Lex(error) => error.into_codec_error(),
            error @ Self::Syntax { .. } => CodecError::Malformed(ctx.format_retained(format_args!("{error}"), "STEP parse error")?),
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
        while !self.peek_name("ENDSEC")? {
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
            if self.peek_name("ANCHOR")? || self.peek_name("REFERENCE")? {
                let (message, _message_storage) = self.budget.format_scoped(format_args!("{level} forbids ANCHOR and REFERENCE sections"), "STEP forbidden section message")?;
                return self.err(&message);
            }
        }
        if self.peek_name("ANCHOR")? {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC")? {
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
                tags.shrink_to_fit();

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
        if self.peek_name("REFERENCE")? {
            self.lexer.set_allow_print_controls(false);
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            while !self.peek_name("ENDSEC")? {
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
                external_storage.with_storage(|| {
                    self.budget
                        .insert_btree_set(same_kind, id, "step_parse_external_reference_ids")
                })?;
                if other_kind.contains(&id) {
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
        while self.peek_name("DATA")? {
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
            while !self.peek_name("ENDSEC")? {
                let (id, record) = self.record()?;

                if records.contains_key(&id) {
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
            ids.shrink_to_fit();

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
        match validate_header_data_references(self.budget, &header_data_references, &data_section_names) {
            Ok(()) => {},
            Err(ValidationError::Invalid(message)) => return self.err(message),
            Err(ValidationError::Resource(error)) => return Err(ParseError::Resource(error)),
        }
        self.name("END-ISO-10303-21")?;
        self.punct(&TokenKind::Semicolon)?;
        let mut signatures = Vec::new();
        if let Some(level) = implementation_level.edition3_sections_forbidden_by() {
            if self.peek_name("SIGNATURE")? {
                let (message, _message_storage) = self.budget.format_scoped(format_args!("{level} forbids SIGNATURE sections"), "STEP forbidden section message")?;
                return self.err(&message);
            }
        }
        while self.peek_name("SIGNATURE")? {
            let start = self.current_offset();
            self.next_kind()?;
            self.punct(&TokenKind::Semicolon)?;
            let payload_start = self.last_end;
            let payload_end = self.current_offset();
            while !self.peek_name("ENDSEC")? {
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
            self.budget
                .push_vec(&mut signatures, span, "step_parse_signature_spans")?;
        }
        if self.current.is_some() {
            return self.err("tokens after exchange terminator");
        }
        if self.budget.admit_iter(&(records), "STEP exchange map traversal").map_err(cadmpeg_core::CodecError::from)?.map(|(key, _)| key).any(|id| external_reference_ids.contains(id)) {
            return self.err("external reference instance collides with a DATA instance");
        }
        if self.budget.admit_iter(&(records), "STEP exchange map traversal").map_err(cadmpeg_core::CodecError::from)?.map(|(key, _)| key)
            .any(|id| external_value_reference_ids.contains(id))
        {
            return self.err("external value instance collides with a DATA instance");
        }
        if !anchors.is_empty() {
            let mut binding_storage = self
                .budget
                .reserve_scoped(0, "step_anchor_binding_storage")?;
            let mut anchor_bindings = BTreeMap::new();
            for anchor in self.budget.admit_iter(&(anchors)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)? {
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
        let class3_restriction = if let Some(restriction) = implementation_level.class3_occurrence_restriction() {
            let contains = 'occurrence: {
                for record in self.budget.admit_iter(header.as_slice(), "STEP class-3 header traversal").map_err(CodecError::from)? {
                    for value in self.budget.admit_iter(record.parameters.as_slice(), "STEP class-3 header parameters").map_err(CodecError::from)? {
                        if contains_class3_occurrence(self.budget, value)? { break 'occurrence true; }
                    }
                }
                for anchor in self.budget.admit_iter(anchors.as_slice(), "STEP class-3 anchor traversal").map_err(CodecError::from)? {
                    if contains_class3_occurrence(self.budget, &anchor.value)? { break 'occurrence true; }
                    for tag in self.budget.admit_iter(anchor.tags.as_slice(), "STEP class-3 anchor tags").map_err(CodecError::from)? {
                        if contains_class3_occurrence(self.budget, &tag.value)? { break 'occurrence true; }
                    }
                }
                for (_, record) in self.budget.admit_iter(&records, "STEP class-3 record traversal").map_err(CodecError::from)? {
                    for partial in self.budget.admit_iter(&record.partials[..], "STEP class-3 partial traversal").map_err(CodecError::from)? {
                        for value in self.budget.admit_iter(partial.parameters.as_slice(), "STEP class-3 record parameters").map_err(CodecError::from)? {
                            if contains_class3_occurrence(self.budget, value)? { break 'occurrence true; }
                        }
                    }
                }
                false
            };
            contains.then_some(restriction)
        } else { None };
        resolve_local_references(&mut anchors, &mut records, &reference_entries, self.budget)
            .map_err(|error| error.into_parse_error(0))?;
        for record in records.values_mut() {
            if record.partials.len() == 1 && omitted_entity_name(&record.partials[0]) {
                let parameters = &mut record.partials[0].parameters;

                self.budget
                    .reserve_vec(parameters, 1, "step_omitted_name_recovery_item")?;
                record.partials[0]
                    .parameters
                    .insert(0, Value::String(Vec::new()));
                record.partials[0].parameters.shrink_to_fit();
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
        for anchor in self.budget.admit_iter(&(anchors)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)? {
            refs.clear();
            value_refs.clear();
            reference_storage.with_storage(|| {
                references(&anchor.value, &mut refs, &mut value_refs, self.budget)
            })?;
            if self.budget.admit_iter(&(refs)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)?
                .any(|id| !records.contains_key(id) && !external_reference_ids.contains(id))
            {
                return self.err("unresolved instance reference in anchor binding");
            }
            if self.budget.admit_iter(&(value_refs)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)?
                .any(|id| !external_value_reference_ids.contains(id))
            {
                return self.err("unresolved value instance reference in anchor binding");
            }
            for tag in self.budget.admit_iter(&(anchor.tags)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)? {
                refs.clear();
                value_refs.clear();
                reference_storage.with_storage(|| {
                    references(&tag.value, &mut refs, &mut value_refs, self.budget)
                })?;
                if self.budget.admit_iter(&(refs)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)?
                    .any(|id| !records.contains_key(id) && !external_reference_ids.contains(id))
                {
                    return self.err("unresolved instance reference in anchor tag");
                }
                if self.budget.admit_iter(&(value_refs)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)?
                    .any(|id| !external_value_reference_ids.contains(id))
                {
                    return self.err("unresolved value instance reference in anchor tag");
                }
            }
        }
        for record in self.budget.admit_iter(&(records), "STEP exchange map traversal").map_err(cadmpeg_core::CodecError::from)?.map(|(_, value)| value) {
            refs.clear();
            value_refs.clear();
            for partial in self.budget.admit_iter(&(record.partials)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)? {
                for value in self.budget.admit_iter(&(partial.parameters)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)? {
                    reference_storage.with_storage(|| {
                        references(value, &mut refs, &mut value_refs, self.budget)
                    })?;
                }
            }
            if self.budget.admit_iter(&(refs)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)?
                .any(|id| !records.contains_key(id) && !external_reference_ids.contains(id))
            {
                return Self::err_at(self.budget, record.span.start, "unresolved instance reference");
            }
            if self.budget.admit_iter(&(value_refs)[..], "STEP exchange traversal").map_err(cadmpeg_core::CodecError::from)?
                .any(|id| !external_value_reference_ids.contains(id))
            {
                return Self::err_at(self.budget, record.span.start, "unresolved value instance reference");
            }
        }
        if let Some(message) = class3_restriction {
            return self.err(message);
        }
        let has_resource_value = 'resource_value: {
            for record in self.budget.admit_iter(header.as_slice(), "STEP resource header traversal").map_err(CodecError::from)? {
                for value in self.budget.admit_iter(record.parameters.as_slice(), "STEP resource header parameters").map_err(CodecError::from)? {
                    if contains_resource_value(self.budget, value)? { break 'resource_value true; }
                }
            }
            for (_, record) in self.budget.admit_iter(&records, "STEP resource record traversal").map_err(CodecError::from)? {
                for partial in self.budget.admit_iter(&record.partials[..], "STEP resource partial traversal").map_err(CodecError::from)? {
                    for value in self.budget.admit_iter(partial.parameters.as_slice(), "STEP resource record parameters").map_err(CodecError::from)? {
                        if contains_resource_value(self.budget, value)? { break 'resource_value true; }
                    }
                }
            }
            false
        };
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
        header.shrink_to_fit();
        anchors.shrink_to_fit();
        reference_entries.shrink_to_fit();
        data.shrink_to_fit();
        signatures.shrink_to_fit();
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
                let partial = self.partial()?;
                self.budget
                    .push_vec(&mut parts.0, partial, "step_parse_record_partials")?;
            }
            self.next_kind()?;
            let mut name_storage = self.budget.reserve_scoped(0, "step partial name lookup")?;
            let mut canonical_names = Vec::new();
            for part in self.budget.admit_iter(&(parts)[..], "STEP record traversal").map_err(cadmpeg_core::CodecError::from)? {
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
            let pair_width = std::num::NonZeroUsize::new(2)
                .ok_or_else(|| self.budget.refuse_codec_limit("STEP complex partial pair width", 0, 1))?;
            for window in self.budget.admit_iter(canonical_names.as_slice(), "STEP complex partial pair traversal").map_err(CodecError::from)?.windows(pair_width) {
                if self.budget.equal(window[0], window[1], "STEP complex partial name equality")? {
                    return Self::err_at(self.budget, start, "duplicate complex partial name");
                }
            }
            if !parts
                .windows(2)
                .all(|window| window[0].name < window[1].name)
            {
                let message = self.budget.format_retained(format_args!(
                        "complex partial records are not alphabetical: observed ({}), expected ({})",
                        PartialNameList(&parts),
                        SortedPartialNameList(&canonical_names),
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
        partials.0.shrink_to_fit();
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
        if self.budget.equal(actual.as_str(), expected, "STEP expected parser name comparison")? {
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
    fn peek_name(&self, expected: &str) -> Result<bool, ParseError> {
        match self.current.as_ref().map(|token| &token.kind) {
            Some(TokenKind::Name(name)) => self.budget
                .equal(name.as_str(), expected, "STEP parser lookahead name equality")
                .map_err(ParseError::from),
            _ => Ok(false),
        }
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
            self.budget.charge_work(1, "step_lex_token")?;
        }
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
    for (record, expected) in [description_record, file_name_record, schema_record].into_iter().zip(REQUIRED) {
        if !budget.equal(record.name.as_str(), expected, "STEP required header order equality")? {
            return invalid("HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA");
        }
    }
    for name in REQUIRED {
        let mut count = 0usize;
        for record in budget.admit_iter(header, "STEP required header occurrence traversal").map_err(CodecError::from)? {
            if budget.equal(record.name.as_str(), name, "STEP required header occurrence equality")? {
                count = count.checked_add(1)
                    .ok_or_else(|| budget.refuse_codec_limit("STEP required header occurrence count", 0, 1))?;
            }
        }
        if count != 1 {
            return invalid("HEADER contains a duplicate required entity");
        }
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
    let Some(time_stamp) = decoded_string(file_name_timestamp, implementation_level, budget)?
    else {
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
    let mut normalized_identifiers = BTreeSet::new();
    for value in identifiers {
        let Value::String(bytes) = value else {
            return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
        };
        let Some(identifier) = decoded_bytes(bytes, implementation_level, budget)? else {
            return invalid("FILE_SCHEMA has invalid or duplicate schema identifiers");
        };
        let trimmed = identifier.trim();
        budget.charge_retained(
            u64_from_index(trimmed.len()),
            "step_schema_identifier_normalized",
        )?;
        let normalized = trimmed.to_ascii_uppercase();
        if !budget.insert_btree_set(
            &mut normalized_identifiers,
            normalized,
            "step_schema_identifier_names",
        )? {
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
    admitted
        .iter()
        .filter_map(move |identifier| identifier.out_of_range().map(|(name, component)| budget.format_retained(format_args!(
                    "FILE_SCHEMA identifier {name} has an out-of-range object identifier component {component}; the object identifier is not admitted"
                ), "step_schema_oid_diagnostic_text")
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
    budget: &DecodeContext<'_>,
) -> Result<Vec<HeaderDataReferences>, ValidationError> {
    let has = |name: &str| -> Result<bool, CodecError> {
        for record in budget.admit_iter(header, "STEP validate header sections traversal").map_err(CodecError::from)? {
            if budget.equal(record.name.as_str(), name, "STEP header section name equality")? {
                return Ok(true);
            }
        }
        Ok(false)
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
    let mut language_sections = BTreeSet::new();
    let mut context_sections = BTreeSet::new();
    for record in budget.admit_iter(&(header)[..], "STEP validate header sections traversal").map_err(cadmpeg_core::CodecError::from)?.skip(3) {
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
                let section_copy = section
                    .as_deref()
                    .map(|value| {
                        budget.copy_retained_text(value, "step_section_language_name_copy")
                    })
                    .transpose()
                    .map_err(ValidationError::Resource)?;
                if !budget.insert_btree_set(
                    &mut language_sections,
                    section_copy,
                    "step_section_language_names",
                )? {
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
                let section_copy = section
                    .as_deref()
                    .map(|value| budget.copy_retained_text(value, "step_section_context_name_copy"))
                    .transpose()
                    .map_err(ValidationError::Resource)?;
                if !budget.insert_btree_set(
                    &mut context_sections,
                    section_copy,
                    "step_section_context_names",
                )? {
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
    for identification in identifications {
        let Value::List(values) = identification else {
            return Ok(false);
        };
        let [Value::String(address), time_stamp, digest] = values.as_slice() else {
            return Ok(false);
        };
        if decoded_bytes(address, implementation_level, budget)?.is_none()
            || !valid_optional_timestamp(time_stamp, implementation_level, budget)?
            || !valid_optional_base64(digest, implementation_level, budget)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn admit_file_population(
    parameters: &[Value],
    schema_identifiers: &[String],
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<BTreeSet<String>, ValidationError> {
    let [Value::String(schema), Value::String(determination), governed_sections] = parameters
    else {
        return invalid("FILE_POPULATION has invalid parameters");
    };
    let Some(schema) = decoded_bytes(schema, implementation_level, budget)? else {
        return invalid("FILE_POPULATION has invalid parameters");
    };
    if !valid_schema_identifier(budget, &schema)?
        || decoded_bytes(determination, implementation_level, budget)?.is_none()
        || !schema_identifier_matches(schema_identifiers, &schema, budget)?
    {
        return invalid("FILE_POPULATION has invalid parameters");
    }
    match governed_sections {
        Value::Omitted => Ok(BTreeSet::new()),
        Value::List(sections) if !sections.is_empty() => {
            let mut names = BTreeSet::new();
            for section in sections {
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
    let Some(language) = decoded_string(language, implementation_level, budget)? else {
        return invalid("SECTION_LANGUAGE has invalid parameters");
    };
    if language.len() != 3 || !budget.admit_iter(language.as_bytes(), "STEP section language validation").map_err(CodecError::from)?.all(|byte| byte.is_ascii_alphabetic()) {
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
    for context in contexts {
        if decoded_string(context, implementation_level, budget)?.is_none() {
            return invalid("SECTION_CONTEXT has invalid parameters");
        }
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
    Ok(decoded_string(value, implementation_level, budget)?.is_some())
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
    for value in values {
        if !is_decodable_string(value, implementation_level, budget)? {
            return Ok(false);
        }
    }
    Ok(true)
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
    match decoded_string(value, implementation_level, budget)? {
        Some(value) => Ok(budget.admit_iter(value.as_str(), "STEP string length traversal")?.count() <= limit),
        None => Ok(false),
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
    for value in values {
        if !string_within_limit(value, implementation_level, limit, budget)? {
            return Ok(false);
        }
    }
    Ok(true)
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
        Value::String(_) => match decoded_string(value, implementation_level, budget)? {
            Some(value) => valid_timestamp_text(budget, &value),
            None => Ok(false),
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
        || !all_ascii_digits(budget, &bytes[0..4])?
        || !all_ascii_digits(budget, &bytes[5..7])?
        || !all_ascii_digits(budget, &bytes[8..10])?
        || !all_ascii_digits(budget, &bytes[11..13])?
        || !all_ascii_digits(budget, &bytes[14..16])?
        || !all_ascii_digits(budget, &bytes[17..19])?
    {
        return Ok(false);
    }
    let year = parse_ascii_digits(budget, &bytes[0..4])?;
    let month = parse_ascii_digits(budget, &bytes[5..7])?;
    let day = parse_ascii_digits(budget, &bytes[8..10])?;
    let hour = parse_ascii_digits(budget, &bytes[11..13])?;
    let minute = parse_ascii_digits(budget, &bytes[14..16])?;
    let second = parse_ascii_digits(budget, &bytes[17..19])?;
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
                && all_ascii_digits(budget, &bytes[at..at + 2])?
                && bytes[at + 2] == b':'
                && all_ascii_digits(budget, &bytes[at + 3..at + 5])?
                && parse_ascii_digits(budget, &bytes[at..at + 2])? <= 23
                && parse_ascii_digits(budget, &bytes[at + 3..at + 5])? <= 59
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

fn all_ascii_digits(budget: &DecodeContext<'_>, bytes: &[u8]) -> Result<bool, CodecError> {
    Ok(!bytes.is_empty() && budget.admit_iter(bytes, "STEP decimal digit validation")?.all(u8::is_ascii_digit))
}

fn parse_ascii_digits(budget: &DecodeContext<'_>, bytes: &[u8]) -> Result<usize, CodecError> {
    budget.admit_iter(bytes, "STEP decimal digit accumulation")?
        .try_fold(0_usize, |value, byte| value.checked_mul(10)
            .and_then(|value| value.checked_add(usize::from(byte - b'0')))
            .ok_or_else(|| budget.refuse_codec_limit("STEP decimal digit accumulation", u64::MAX, u64::MAX)))
}

fn valid_optional_base64(
    value: &Value,
    implementation_level: ImplementationLevel,
    budget: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    match value {
        Value::Omitted => Ok(true),
        Value::String(_) => match decoded_string(value, implementation_level, budget)? {
            Some(value) => valid_base64_text(budget, value.as_bytes()),
            None => Ok(false),
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
    for &byte in budget.admit_iter(bytes, "STEP base64 validation traversal")? {
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
    let trimmed = schema_name.trim();
    let (mut schema_name, _storage) = budget
        .with_scoped_storage("step_schema_name_matching", || {
            budget.copy_retained_text(trimmed, "step_schema_name_matching")
        })?;
    schema_name.make_ascii_uppercase();
    for identifier in budget.admit_iter(schema_identifiers, "STEP schema identifier matches traversal")? {
        let identifier = identifier.trim();
        if budget.equal(identifier, schema_name.as_str(), "STEP full schema identifier equality")? {
            return Ok(true);
        }
        if let Some((name, _)) = split_schema_identifier(identifier) {
            if budget.equal(name, schema_name.as_str(), "STEP schema identifier prefix equality")? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn validate_header_data_references(
    budget: &DecodeContext<'_>,
    references: &[HeaderDataReferences],
    data_section_names: &BTreeSet<String>,
) -> Result<(), ValidationError> {
    for reference in budget.admit_iter(references, "STEP header DATA reference traversal").map_err(CodecError::from)? {
        match reference {
            HeaderDataReferences::FilePopulation(sections) => {
                for section in budget.admit_iter(sections, "STEP FILE_POPULATION section traversal").map_err(CodecError::from)? {
                    if !budget.contains_btree_set(data_section_names, section, "STEP DATA section name lookup")? {
                        return invalid("FILE_POPULATION names an unknown DATA section");
                    }
                }
            }
            HeaderDataReferences::Section(section) => {
                if !budget.contains_btree_set(data_section_names, section, "STEP DATA section name lookup")? {
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
    let Some(schema_name) = decoded_bytes(schema_name, implementation_level, budget)? else {
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
    for identifier in budget.admit_iter(admitted, "STEP schema names for matching traversal").map_err(cadmpeg_core::CodecError::from)? {
        let source = identifier.text();
        let mut name = budget.copy_retained_text(source, "step_schema_matching_name")?;
        name.make_ascii_uppercase();
        budget.push_vec(&mut names, name, "step_schema_matching_names")?;
    }
    Ok(names)
}

fn is_string_list(budget: &DecodeContext<'_>, value: Option<&Value>) -> Result<bool, CodecError> {
    match value {
        Some(Value::List(values)) if !values.is_empty() => Ok(budget.admit_iter(values.as_slice(), "STEP string list traversal")?.all(|value| matches!(value, Value::String(_)))),
        _ => Ok(false),
    }
}

fn is_string_or_omitted(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::String(_) | Value::Omitted))
}

fn valid_anchor_name(budget: &DecodeContext<'_>, name: &str) -> Result<bool, CodecError> {
    Ok(!name.is_empty() && budget.admit_iter(name.as_bytes(), "STEP anchor name traversal")?.any(|byte| !byte.is_ascii_digit()))
}

fn is_anchor_item(budget: &DecodeContext<'_>, value: &Value) -> Result<bool, CodecError> {
    let _depth = budget.enter_nested("STEP anchor item classification")?;
    match value {
        Value::Reference(_) | Value::ExternalReference(_) | Value::ConstantEntity(_)
        | Value::ExpressValueConstant(_) | Value::Integer(_) | Value::Real(_)
        | Value::Enumeration(_) | Value::String(_) | Value::Binary(_)
        | Value::Resource(_) | Value::Omitted => Ok(true),
        Value::List(values) => {
            for value in budget.admit_iter(values.as_slice(), "STEP anchor item traversal")? {
                if !is_anchor_item(budget, value)? { return Ok(false); }
            }
            Ok(true)
        }
        Value::Derived | Value::Typed(_, _) => Ok(false),
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
            if let Some((name, source)) = self.anchors.get_key_value(name) {
                let name = name.as_str();
                if let Some((value, nodes)) = self.memo.get(name) {
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
                if stack.contains(&name) {
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
                        self.budget.admit_btree_entry(
                            &self.memo,
                            &name,
                            "step_anchor_memo_entry",
                        )?;
                        self.memo.insert(
                            name,
                            (
                                try_clone_value(
                                    &value,
                                    self.budget,
                                    "step_anchor_memo_value_copy",
                                )?,
                                nodes,
                            ),
                        );
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
                for value in self.budget.admit_iter(values.as_slice(), "STEP resolve value traversal").map_err(cadmpeg_core::CodecError::from)? {
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
                    .charge_work(u64_from_index(name.len()), "step_anchor_typed_name_copy")?;
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
        for reference in budget.admit_iter(references, "STEP new traversal").map_err(cadmpeg_core::CodecError::from)? {
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
                for value in self.budget.admit_iter(values.as_slice(), "STEP resolve value value traversal").map_err(cadmpeg_core::CodecError::from)? {
                    resolved.push(self.resolve_value(value, depth + 1)?);
                }
                Ok(Value::List(resolved))
            }
            Value::Typed(name, value) => {
                self.consume_materialized_node()?;
                let resolved = self.resolve_value(value, depth + 1)?;
                self.budget
                    .charge_work(u64_from_index(name.len()), "step_reference_typed_name_copy")?;
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
        let Some(anchor) = self.anchors.get(fragment) else {
            return if is_uuid_fragment(self.budget, fragment)? {
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
    for anchor in budget.admit_iter(&(anchors)[..], "STEP resolve local references traversal").map_err(cadmpeg_core::CodecError::from)? {
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

fn reference_target_matches(name: ReferenceName, value: &Value) -> bool {
    match name {
        ReferenceName::Entity(_) => matches!(value, Value::Reference(_) | Value::ConstantEntity(_)),
        ReferenceName::Value(_) => !matches!(
            value,
            Value::Reference(_) | Value::ConstantEntity(_) | Value::Resource(_)
        ),
    }
}

fn is_uuid_fragment(budget: &DecodeContext<'_>, fragment: &str) -> Result<bool, CodecError> {
    Ok(fragment.len() == 36
        && budget.admit_iter(fragment.as_bytes(), "STEP UUID fragment traversal")?.enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                .then_some(*byte == b'-')
                .unwrap_or_else(|| byte.is_ascii_hexdigit())
        }))
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
                for child in budget.admit_iter(values.as_slice(), "STEP visit value traversal").map_err(cadmpeg_core::CodecError::from)? {
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
    let mut pending = Vec::new();
    budget.push_vec(&mut pending, value, "step_parse_reference_pending")?;
    while let Some(value) = pending.pop() {
        match value {
            Value::Reference(id) => {
                budget.push_vec(entity_out, *id, "step_parse_reference_ids")?;
            }
            Value::ExternalReference(id) => {
                budget.push_vec(value_out, *id, "step_parse_value_reference_ids")?;
            }
            Value::List(values) => {
                for child in budget.admit_iter(&(values)[..], "STEP references traversal").map_err(cadmpeg_core::CodecError::from)?.rev() {
                    budget.push_vec(&mut pending, child, "step_parse_reference_pending")?;
                }
            }
            Value::Typed(_, value) => {
                budget.push_vec(&mut pending, value, "step_parse_reference_pending")?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn contains_class3_occurrence(budget: &DecodeContext<'_>, value: &Value) -> Result<bool, CodecError> {
    let _depth = budget.enter_nested("STEP class-3 occurrence classification")?;
    match value {
        Value::ExternalReference(_) | Value::ConstantEntity(_) | Value::ExpressValueConstant(_) => Ok(true),
        Value::List(values) => {
            for value in budget.admit_iter(values.as_slice(), "STEP class-3 occurrence traversal")? {
                if contains_class3_occurrence(budget, value)? { return Ok(true); }
            }
            Ok(false)
        }
        Value::Typed(_, value) => contains_class3_occurrence(budget, value),
        _ => Ok(false),
    }
}

fn contains_resource_value(budget: &DecodeContext<'_>, value: &Value) -> Result<bool, CodecError> {
    let _depth = budget.enter_nested("STEP resource value classification")?;
    match value {
        Value::Resource(_) => Ok(true),
        Value::List(values) => {
            for value in budget.admit_iter(values.as_slice(), "STEP resource value traversal")? {
                if contains_resource_value(budget, value)? { return Ok(true); }
            }
            Ok(false)
        }
        Value::Typed(_, value) => contains_resource_value(budget, value),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests;
