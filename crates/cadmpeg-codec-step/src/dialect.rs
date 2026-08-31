// SPDX-License-Identifier: Apache-2.0
//! STEP dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, `DialectId::pinned` strings
//! are the boundary, [`StepDialect::classify`] is the one construction path for
//! a [`DialectMatch`], and the vocabulary is closed. Tests close it directly
//! against `docs/dialects.toml`.
//!
//! # The identity axis is the `FILE_SCHEMA` identifier
//!
//! Identity rows and parser grammars are independent, and STEP is the extreme
//! case of the gap. The codec has exactly one Part 21 read grammar:
//! nothing in the reader branches on the declared schema. The schema identifier
//! is read after the parse, recorded, and used for DATA-section name matching.
//! It is nonetheless the identity axis. [`StepSchema::file_schema`] owns the
//! declarations for writable Part 21 rows; [`StepDialect::schema_identifier`]
//! delegates those rows to it and adds the edition-unspecified AP242 row.
//!
//! The `FILE_DESCRIPTION` implementation level is the axis the parser does
//! branch on, and it is deliberately **not** an identity axis here: it is
//! recorded verbatim under [`DECLARED_IMPLEMENTATION_LEVEL`] and nothing in
//! this module reads it. Crossing the two axes would make the rows disjoint at
//! the cost of about 45 rows, most of them unwitnessed.
//!
//! # The declaration is evidence; the id is identity
//!
//! [`DialectMatch::declared`] records what `FILE_SCHEMA` says.
//! [`DialectMatch::dialect`] records which registry row the document satisfies.
//! A row matches only when *every* one of its discriminants matches. Four rows
//! share the AP242 schema name and separate on the object identifier, which
//! Part 21 makes optional: absent is [`StepDialect::Ap242`], the region where
//! the schema is declared and the edition is not; each declared edition has its
//! own row; an edition claim naming no declared edition satisfies no row and
//! lands on [`StepDialect::Unknown`], the mandatory totality row. Parsing an
//! edition out of an id, or expecting an id to agree with the
//! identifier beside it, is wrong for exactly the files whose declarations are
//! incomplete.
//!
//! # Alternate encodings
//!
//! Three rows — Part 28 XML, the AP242 BO-Model XML sidecar, and the Part 26
//! HDF5 encoding — are structural refusals below decode.
//! [`refuse_alternate_encoding`] returns [`CodecError::UnsupportedDialect`]
//! before any exchange structure is read. The error carries the refused row's
//! [`DialectMatch`] even though no decode report exists.

use crate::loss::StepLossCode;
use crate::options::StepSchema;
use crate::parse::schema_identifier::split_schema_identifier;
use crate::parse::{Exchange, Value};
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "step";

/// Key of the `FILE_SCHEMA` identifier the row keys on, in
/// [`DialectMatch::declared`].
///
/// Verbatim as read, object-identifier braces included. Absent when the header
/// declares no readable `FILE_SCHEMA` identifier at all.
pub(crate) const DECLARED_FILE_SCHEMA_IDENTIFIER: &str = "file_schema_identifier";
/// Key of the whole `FILE_SCHEMA` list, in [`DialectMatch::declared`], present
/// only when the list declares more than one identifier.
///
/// The identifiers verbatim as read, joined with `,`. See
/// [`StepDialect::classify`] for why one entry of the list is the identity.
pub(crate) const DECLARED_FILE_SCHEMA_IDENTIFIERS: &str = "file_schema_identifiers";
/// Key of the object-identifier arcs of the classified identifier, in
/// [`DialectMatch::declared`]. Absent when the identifier carries none.
///
/// The text between the braces, verbatim as read and untrimmed, so
/// `'… { 1 0 10303 442 3 1 4 }'` records `" 1 0 10303 442 3 1 4 "`. This is
/// evidence, not a join key: `docs/dialects.toml` writes the same arcs trimmed
/// under `long_form_arcs`, and the resolved id is what a consumer compares.
pub(crate) const DECLARED_LONG_FORM_ARCS: &str = "long_form_arcs";
/// Key of the `FILE_DESCRIPTION` implementation level, in
/// [`DialectMatch::declared`]. Absent when the header declares no readable one.
///
/// Verbatim as read: `"2;1"`, `"4;2"`. Evidence only. The parser branches on
/// this value (`crate::parse::ImplementationLevel`) but no row here does.
pub(crate) const DECLARED_IMPLEMENTATION_LEVEL: &str = "implementation_level";

/// One row of `docs/dialects.toml` under the `step` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StepDialect {
    Schema(StepSchema),
    Ap242,
    Part28Xml,
    Ap242BoModelXml,
    Part26Hdf5,
    Unknown,
}

/// The row whose entity vocabulary this codec actually reads a Part 21
/// exchange with, whatever the exchange declares.
///
/// There is one Part 21 grammar and one entity vocabulary here, and it is the
/// AP242 one: tessellation, semantic PMI, and draughting presentation are
/// decoded from any exchange, and no reader stage consults `FILE_SCHEMA`.
/// Edition 3 is the newest of the AP242 rows and the one the reader's other
/// defaults follow, so it names the strategy used.
const NEAREST_STRATEGY: StepDialect = StepDialect::Schema(StepSchema::Ap242Edition3);

impl StepDialect {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 11] = [
        Self::Schema(StepSchema::Ap203Edition1),
        Self::Schema(StepSchema::Ap203Edition2),
        Self::Schema(StepSchema::Ap214),
        Self::Ap242,
        Self::Schema(StepSchema::Ap242Edition1),
        Self::Schema(StepSchema::Ap242Edition2),
        Self::Schema(StepSchema::Ap242Edition3),
        Self::Part28Xml,
        Self::Ap242BoModelXml,
        Self::Part26Hdf5,
        Self::Unknown,
    ];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(self.pinned())
    }

    const fn pinned(self) -> &'static str {
        match self {
            Self::Schema(schema) => schema.target(),
            Self::Ap242 => "step:ap242",
            Self::Part28Xml => "step:part28-xml",
            Self::Ap242BoModelXml => "step:ap242-bo-model-xml",
            Self::Part26Hdf5 => "step:part26-hdf5",
            Self::Unknown => "step:unknown",
        }
    }

    /// The canonical `FILE_SCHEMA` identifier for a Part 21 identity row.
    ///
    /// The edition-unspecified AP242 row has its legal bare schema name. The
    /// alternate encodings and totality row have no Part 21 declaration.
    pub(crate) const fn schema_identifier(self) -> Option<&'static str> {
        match self {
            Self::Schema(schema) => Some(schema.file_schema()),
            Self::Ap242 => Some("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"),
            Self::Part28Xml | Self::Ap242BoModelXml | Self::Part26Hdf5 | Self::Unknown => None,
        }
    }

    /// The row for one `FILE_SCHEMA` identifier, or [`Self::Unknown`] where the
    /// registry declares no such row.
    ///
    /// The schema name is the first discriminant of every Part 21 row, and the
    /// three single-row names need nothing else: their arcs, present or absent,
    /// separate no two rows. This matches the reader's own bare-or-arc-bearing
    /// name comparison (`crate::parse::schema_identifier_matches`).
    ///
    /// The AP242 name is shared by four rows, so the object identifier is the
    /// second discriminant there and its three states are distinct facts:
    ///
    /// - absent — a complete declaration naming no edition, [`Self::Ap242`].
    ///   The object identifier is optional in Part 21; leaving it out is legal
    ///   and says the edition is unspecified, not that the file is unrecognized.
    /// - present and naming an edition — that edition's row, decided against
    ///   this enum's own canonical identifiers.
    /// - present and naming no edition — [`Self::Unknown`]. An edition claim
    ///   that matches nothing this codec declares is an unrecognized
    ///   declaration, unlike making no claim at all. Arcs that do not read as a
    ///   numeric object identifier reach the same place, through the same call.
    fn from_schema_identifier(identifier: &str) -> Self {
        let Some((name, object_identifier)) = split_schema_identifier(identifier) else {
            return Self::Unknown;
        };
        let ap242_name = Self::Ap242
            .schema_identifier()
            .expect("the AP242 row has a Part 21 identifier");
        if name.eq_ignore_ascii_case(ap242_name) {
            if object_identifier.is_none() {
                return Self::Ap242;
            }
            return Self::from_ap242_identifier(identifier).unwrap_or(Self::Unknown);
        }
        [
            Self::Schema(StepSchema::Ap203Edition1),
            Self::Schema(StepSchema::Ap203Edition2),
            Self::Schema(StepSchema::Ap214),
        ]
        .into_iter()
        .find(|dialect| {
            split_schema_identifier(
                dialect
                    .schema_identifier()
                    .expect("Part 21 discriminants have identifiers"),
            )
            .is_some_and(|(candidate, _)| name.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or(Self::Unknown)
    }

    /// The AP242 edition row whose canonical object identifier the declaration
    /// names. A future or malformed object identifier names no verified row.
    fn from_ap242_identifier(identifier: &str) -> Option<Self> {
        let (name, arcs) = schema_identifier_arcs(identifier)?;
        [
            Self::Schema(StepSchema::Ap242Edition1),
            Self::Schema(StepSchema::Ap242Edition2),
            Self::Schema(StepSchema::Ap242Edition3),
        ]
        .into_iter()
        .find(|dialect| {
            schema_identifier_arcs(
                dialect
                    .schema_identifier()
                    .expect("AP242 edition rows have identifiers"),
            )
            .is_some_and(|(candidate_name, candidate_arcs)| {
                name.eq_ignore_ascii_case(candidate_name) && arcs == candidate_arcs
            })
        })
    }

    /// The refusal message this codec returns for an alternate encoding, or
    /// `None` for a Part 21 row.
    ///
    /// The three alternate-encoding rows are exactly the rows with a message:
    /// they are identified structurally and refused before any exchange
    /// structure is read.
    pub(crate) const fn alternate_encoding_refusal(self) -> Option<&'static str> {
        match self {
            Self::Part26Hdf5 => Some("STEP Part 26 binary/HDF5 encoding"),
            Self::Part28Xml => Some("STEP Part 28 XML encoding"),
            Self::Ap242BoModelXml => Some("AP242 BO-Model XML sidecar"),
            Self::Schema(_) | Self::Ap242 | Self::Unknown => None,
        }
    }

    /// Classifies one structurally identified alternate encoding at refusal.
    fn classify_refused(self) -> DialectMatch {
        debug_assert!(self.alternate_encoding_refusal().is_some());
        DialectMatch::refused(self.id())
    }

    /// Classifies one Part 21 exchange. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification bug and the report
    /// can never disagree.
    ///
    /// Identity is the row whose discriminants the declared identifier
    /// satisfies. Admission follows from that one predicate: a document whose
    /// identifier matched a row was read with the grammar that row declares —
    /// there is only one — and a document on [`Self::Unknown`] was read with a
    /// grammar no row declares for it, which is [`NEAREST_STRATEGY`].
    /// [`dialect_loss`] reads the admission back rather than recomputing it, so
    /// the biconditional the decode policy requires holds by construction
    /// rather than by two authors agreeing.
    ///
    /// A `FILE_SCHEMA` list declaring several identifiers is legal Part 21 and
    /// has no representation in [`DialectMatch`], which holds one dialect per
    /// format layer. The first identifier is the identity, matching the dialect
    /// note the codec reports, and the whole list is recorded under
    /// [`DECLARED_FILE_SCHEMA_IDENTIFIERS`].
    pub(crate) fn classify(exchange: &Exchange) -> DialectMatch {
        let identifiers = crate::reader::schema_identifiers(exchange);
        let dialect = identifiers.first().map_or(Self::Unknown, |identifier| {
            Self::from_schema_identifier(identifier)
        });

        let mut declared = BTreeMap::new();
        if let Some(identifier) = identifiers.first() {
            declared.insert(DECLARED_FILE_SCHEMA_IDENTIFIER.into(), identifier.clone());
            if let Some((_, Some(arcs))) = split_schema_identifier(identifier) {
                declared.insert(DECLARED_LONG_FORM_ARCS.into(), arcs.into());
            }
        }
        if identifiers.len() > 1 {
            declared.insert(
                DECLARED_FILE_SCHEMA_IDENTIFIERS.into(),
                identifiers.join(","),
            );
        }
        if let Some(level) = implementation_level(exchange) {
            declared.insert(DECLARED_IMPLEMENTATION_LEVEL.into(), level);
        }

        if dialect == Self::Unknown {
            DialectMatch::unverified(dialect.id(), NEAREST_STRATEGY.id())
        } else {
            DialectMatch::admitted(dialect.id())
        }
        .with_declared(declared)
    }
}

/// The schema name and numeric object-identifier arcs of one declaration.
///
/// Named components, fewer than two components, and non-decimal components do
/// not form an object identifier that can match a declared edition row.
fn schema_identifier_arcs(identifier: &str) -> Option<(&str, Vec<u64>)> {
    let (name, object_identifier) = split_schema_identifier(identifier)?;
    let object_identifier = object_identifier?;
    if name.is_empty() {
        return None;
    }
    let arcs = object_identifier
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (arcs.len() >= 2).then_some((name, arcs))
}

/// The `FILE_DESCRIPTION` implementation level, verbatim as read.
///
/// `None` when the header declares no `FILE_DESCRIPTION`, when its second
/// parameter is not a string, or when that string does not decode. The parse
/// admits only the five levels `crate::parse::ImplementationLevel` enumerates,
/// so nothing else reaches a report; this function stays total regardless.
fn implementation_level(exchange: &Exchange) -> Option<String> {
    let record = exchange
        .header
        .iter()
        .find(|record| record.name == "FILE_DESCRIPTION")?;
    let Some(Value::String(bytes)) = record.parameters.get(1) else {
        return None;
    };
    crate::strings::decode(bytes).ok()
}

/// The dialect-unverified loss a match requires.
///
/// `None` exactly when `matched.admission` is [`Admission::Admitted`], because
/// this reads that field rather than reclassifying the document. Design §7
/// requires the charge on every [`Admission::AdmittedUnverified`], and this is
/// how STEP satisfies it: the codec decodes an unrecognized schema by recording
/// the string and reading the exchange with the AP242 entity vocabulary
/// anyway, which is a recovery, not a verified read.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    let strategy = match matched.admission() {
        Admission::AdmittedUnverified { using: Some(using) } => {
            format!("with the entity vocabulary verified for {using}")
        }
        Admission::AdmittedUnverified { using: None } => {
            "without substituting a declared STEP entity vocabulary".to_owned()
        }
        Admission::Admitted | Admission::Refused => return None,
    };
    let declaration = matched
        .declared()
        .get(DECLARED_FILE_SCHEMA_IDENTIFIER)
        .map_or_else(
            || "The exchange declares no FILE_SCHEMA identifier".to_owned(),
            |identifier| format!("FILE_SCHEMA identifier {identifier}"),
        );
    Some(StepLossCode::SourceDialectUnverified.note(format!(
        "{declaration}; it satisfies no declared STEP dialect, so this decode read the exchange \
{strategy}"
    )))
}

/// Refuses the three alternate encodings this codec identifies and does not
/// decode.
///
/// Structural refusal below decode: it runs before the ISO-10303-21 magic test
/// and returns before any exchange structure exists, so no report is produced.
/// The error carries the structurally identified row as a [`DialectMatch`].
///
/// CE-03/CE-04: Part 28 marker detection is not UOS conformance or schema
/// mapping. The caller owns the exact binding, governing EXPRESS schema,
/// derived XML Schema, identity/reference checks, and validation result; this
/// codec has no Part 28 adapter and builds no partial graph.
/// CE-05: HDF5 signature detection is not HDF5 validation or Part 26 mapping.
/// The caller owns the mapping edition, governing EXPRESS schema, HDF5 and Part
/// 26 validation, resource-local row/reference mapping, and malformed-input
/// result; this codec builds no partial graph.
/// CE-06: Part 26 and Part 21 resource graphs have no universal join. The
/// caller owns the exact resource identities, row-to-occurrence map,
/// schema/unit/context agreement, conflict policy, and retention of both source
/// graphs.
pub(crate) fn refuse_alternate_encoding(bytes: &[u8]) -> Result<(), CodecError> {
    let dialect = if crate::codec::is_part26_hdf5(bytes) {
        StepDialect::Part26Hdf5
    } else if crate::codec::is_part28_xml(bytes) {
        StepDialect::Part28Xml
    } else if crate::codec::is_ap242_bo_model_xml(bytes) {
        StepDialect::Ap242BoModelXml
    } else {
        return Ok(());
    };
    match dialect.alternate_encoding_refusal() {
        Some(message) => Err(CodecError::UnsupportedDialect {
            dialect_match: Box::new(dialect.classify_refused()),
            message: message.into(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
