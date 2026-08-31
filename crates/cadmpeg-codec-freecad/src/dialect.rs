// SPDX-License-Identifier: Apache-2.0
//! `FCStd` dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`FcstdDialect::classify`] is the one construction
//! path, and the vocabulary is closed. Tests close it directly against
//! `docs/dialects.toml`.
//!
//! The discriminant is `Document.xml`'s `SchemaVersion`, read by
//! [`crate::container::parse_document`] before any element vocabulary is
//! chosen. Schema 2 selects `Features`/`FeatureData`/`Feature` and schemas 3
//! and 4 select `Objects`/`ObjectData`/`Object`
//! ([`crate::persistence::parse_with_context`]), so identity has three declared
//! rows and the grammar has two classes. Schemas 3 and 4 are one grammar class
//! and two rows, and both are `Admitted` because a strategy is declared for each.
//!
//! # `FileVersion` is provenance, not evidence
//!
//! `FileVersion` is read and reported and it is half of the writer's target
//! gate, but no decode path branches on it, so no row's discriminants mention
//! it. `ProgramVersion` is metadata throughout. Both travel in
//! [`DialectMatch::declared`] as evidence and nothing reads them to choose a
//! parse.
//!
//! # The declaration is evidence; the id is identity
//!
//! [`DialectMatch::declared`] records what the `Document` element says.
//! [`DialectMatch::dialect`] records which registry row the document satisfies.
//! A `SchemaVersion` of `"04"` parses as the integer 4 and still lands on
//! [`FcstdDialect::Unknown`], because a row matches only when its discriminant
//! matches and no row declares `schema_version = "04"`. Parse a version out of
//! an id, or expect an id to agree with the `schema_version` beside it, and the
//! answer is wrong for exactly the files whose declarations are unusual.

use crate::loss::FreecadLossCode;
use crate::native::DocumentFacts;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "fcstd";

/// The synthesis catalog: the dialects this encoder produces for an input whose
/// retained document graph already declares them.
///
/// Preservation is not listed here. [`crate::writer`] repacks the
/// retained entry set and patches `Document.xml` in place, so it preserves every
/// schema this codec reads — schema 2 included — while it regenerates none. The
/// catalog is what an explicit `--to` may name; [`TargetRequest::Inherit`] asks
/// for the retained document's own dialect instead, whatever that is.
/// No row is a cross-format default because this writer cannot synthesize an
/// `FCStd` document graph from another format.
///
/// [`TargetRequest::Inherit`]: cadmpeg_ir::codec::TargetRequest::Inherit
pub(crate) const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
    id: FcstdDialect::Schema4.id(),
    label: "FreeCAD Document.xml schema version 4",
    aliases: &["4"],
    default: false,
}];

/// Key of `Document/@SchemaVersion` in [`DialectMatch::declared`].
///
/// Verbatim as read. The attribute is required — a document without it is
/// refused as the wrong format before classification — so this key is always
/// present.
const DECLARED_SCHEMA_VERSION: &str = "schema_version";
/// Key of `Document/@FileVersion` in [`DialectMatch::declared`].
///
/// Verbatim as read, except that an absent attribute is recorded as `"0"`,
/// which is the substitution [`crate::container::parse_document`] already makes
/// for the rest of the codec. The key is therefore always present.
const DECLARED_FILE_VERSION: &str = "file_version";
/// Key of `Document/@ProgramVersion` in [`DialectMatch::declared`].
///
/// Verbatim as read, and absent from the map when the attribute is absent from
/// the document: unlike the two above it has no substituted default anywhere in
/// the codec.
const DECLARED_PROGRAM_VERSION: &str = "program_version";

/// One row of `docs/dialects.toml` under the `fcstd` namespace.
///
/// `FreeCAD` publishes no schema-version specification, so `[format.fcstd]`
/// declares `complete = false` and the rows are the schemas the codec's own
/// dispatch enumerates plus the mandatory totality row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FcstdDialect {
    Schema2,
    Schema3,
    Schema4,
    Unknown,
}

impl FcstdDialect {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] = [Self::Schema2, Self::Schema3, Self::Schema4, Self::Unknown];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(self.pinned())
    }

    const fn pinned(self) -> &'static str {
        match self {
            Self::Schema2 => "fcstd:schema-2",
            Self::Schema3 => "fcstd:schema-3",
            Self::Schema4 => "fcstd:schema-4",
            Self::Unknown => "fcstd:unknown",
        }
    }

    /// The row whose discriminant `declared` satisfies, or [`Self::Unknown`]
    /// where the registry declares no such row.
    ///
    /// Keyed on the declared string rather than on a parsed integer, because
    /// the registry's discriminants are strings and a row matches only when its
    /// own discriminant matches.
    pub(crate) fn from_schema_version(declared: &str) -> Self {
        match declared {
            "2" => Self::Schema2,
            "3" => Self::Schema3,
            "4" => Self::Schema4,
            _ => Self::Unknown,
        }
    }

    /// The typed row named by an already-classified dialect id.
    pub(crate) fn from_id(id: &DialectId) -> Option<Self> {
        [Self::Schema2, Self::Schema3, Self::Schema4, Self::Unknown]
            .into_iter()
            .find(|dialect| dialect.id() == *id)
    }

    /// Element vocabulary selected by this persistence identity strategy.
    pub(crate) const fn persistence_tags(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Schema2 => ("Features", "FeatureData", "Feature"),
            _ => ("Objects", "ObjectData", "Object"),
        }
    }

    /// The row whose element vocabulary reads a document this codec declares no
    /// strategy for.
    ///
    /// [`crate::container::parse_document`] maps the declaration once and both
    /// container and persistence parsing match the resulting enum with a
    /// [`Self::Schema2`] arm and an `else`, not a [`Self::Schema3`] or
    /// [`Self::Schema4`] whitelist. Thus, every undeclared schema is scanned with the
    /// `Objects`/`ObjectData`/`Object` vocabulary. Schema 4 is
    /// the newer of the two rows sharing that vocabulary and the one the
    /// writer's own default follows, so it names the strategy used.
    const NEAREST_VERIFIED: Self = Self::Schema4;

    /// Classifies one document. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification bug and the report
    /// can never disagree.
    ///
    /// Reached only after [`crate::container::parse_document`] has accepted the
    /// document element, so `SchemaVersion` is present and reads as an
    /// unsigned integer. `Admission::Refused` is unreachable here: every
    /// decode path, container-only or full, and every inspect reads an
    /// undeclared schema with the `Objects` vocabulary rather than refusing on
    /// the discriminant.
    pub(crate) fn classify(document: &DocumentFacts, dialect: Self) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(
            DECLARED_SCHEMA_VERSION.into(),
            document.schema_version.clone(),
        );
        declared.insert(DECLARED_FILE_VERSION.into(), document.file_version.clone());
        if let Some(version) = &document.program_version {
            declared.insert(DECLARED_PROGRAM_VERSION.into(), version.clone());
        }
        if dialect == Self::Unknown {
            DialectMatch::unverified(dialect.id(), Self::NEAREST_VERIFIED.id())
        } else {
            DialectMatch::admitted(dialect.id())
        }
        .with_declared(declared)
    }

    /// The loss charged when the document's schema names no declared row.
    ///
    /// `None` exactly when the completed match reports
    /// [`Admission::Admitted`].
    pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
        let Admission::AdmittedUnverified { .. } = matched.admission() else {
            return None;
        };
        let schema_version = matched
            .declared()
            .get(DECLARED_SCHEMA_VERSION)
            .map_or("absent", String::as_str);
        Some(FreecadLossCode::SourceDialectUnverified.note(format!(
            "FCStd SchemaVersion={schema_version} names no declared persistence layout; this decode scanned the document with the Objects/ObjectData/Object vocabulary declared for schemas 3 and 4"
        )))
    }
}

#[cfg(test)]
mod tests;
