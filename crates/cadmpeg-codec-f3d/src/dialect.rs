// SPDX-License-Identifier: Apache-2.0
//! F3D dialect identity: which registry row a Fusion archive is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`] strings
//! are the boundary, `F3dDialect::matched` is the one construction path, and
//! the vocabulary is closed. Every variant here has a row in
//! `docs/dialects.toml`; `tests::every_pinned_id_has_a_registry_row_and_every_row_has_a_variant`
//! fails on drift in either direction.
//!
//! # Two grammars, one enum, and a row no run can reach
//!
//! A Fusion ZIP takes one of two document-wide parse strategies, chosen before
//! anything semantic is read (`crate::container::scan`):
//!
//! - A root `Manifest.dat` selects the binary top-level manifest grammar. Its
//!   leading field must equal `3-2-0-0` exactly, and the two fields after it
//!   must equal `FusionDocType` and `.f3d` exactly, so a document that reaches
//!   the rest of the decode is `f3d:manifest-3-2-0-0` by construction.
//! - No root `Manifest.dat`, but `Manifest.json`, `DesignDescription.json`, and
//!   a root-level `*.f3d` member, selects the F3Z multi-document grammar. That
//!   branch reads no version field at all — the test is filename presence — so
//!   `f3d:f3z-multi-document` is an identity row with an unbounded interior.
//!
//! Both are [`Admission::Admitted`]: each is parsed with the strategy its own
//! row declares. [`Admission::AdmittedUnverified`] has **no producer in this
//! codec**. It would need a document read with a grammar its row does not
//! declare, and F3D has no such path: the manifest version is an equality gate,
//! not a clamp, and the F3Z branch declares no version to diverge from. The
//! absence is a fact about F3D's two grammars, not a gap.
//!
//! [`F3dDialect::Unknown`] is the mandatory totality row (design §3.3, B4). Its
//! disposition is refusal: a readable top-level manifest version other than
//! `3-2-0-0` returns `CodecError::NotImplemented` naming the observed version
//! (`crate::manifest::parse_top_level`), and that refusal happens before a
//! [`ContainerSummary`](cadmpeg_core::ContainerSummary) or a `DecodeReport`
//! exists. So the row is declared and pinned, and no [`DialectMatch`] this
//! codec builds ever carries it. Identity surviving refusal is what
//! `CodecError::UnsupportedDialect` will deliver; migrating the refusal is a
//! later phase and is deliberately not done here.

use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "f3d";

/// Key of the top-level `Manifest.dat` version field in
/// [`DialectMatch::declared`], recorded as the manifest cursor read it.
///
/// The value is the length-prefixed ASCII field at the head of the manifest.
/// `crate::manifest::parse_top_level` compares it for exact equality and
/// refuses anything else, so every recorded value is `3-2-0-0` today. The key
/// carries the parse's own reading rather than the constant it matched, so a
/// widening of the gate surfaces here without a second edit.
pub(crate) const DECLARED_TOP_LEVEL_MANIFEST_VERSION: &str = "top_level_manifest_version";

/// Key of the root-level `*.f3d` member names in [`DialectMatch::declared`],
/// comma-separated and sorted by archive path.
///
/// An F3Z archive declares no version anywhere: the branch that identifies it
/// tests for the absence of `Manifest.dat` and the presence of `Manifest.json`,
/// `DesignDescription.json`, and at least one root-level `*.f3d`. The first
/// three are constants and carry no information about the document. The
/// root-level member names are the one part of that discriminant the source
/// authored, and each is recorded verbatim as the archive spells it.
pub(crate) const DECLARED_ROOT_DOCUMENT_MEMBERS: &str = "root_document_members";

/// Separator between root-level member names in
/// [`DECLARED_ROOT_DOCUMENT_MEMBERS`].
const MEMBER_SEPARATOR: &str = ",";

/// One row of `docs/dialects.toml` under the `f3d` namespace.
///
/// Three variants is still an enum: the drift test against the registry is what
/// the type is for, and it holds at three rows exactly as it holds at
/// twenty-two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum F3dDialect {
    /// Root `Manifest.dat` whose version, kind, and extension fields equal
    /// `3-2-0-0`, `FusionDocType`, and `.f3d`.
    Manifest3200,
    /// No root `Manifest.dat`; the F3Z manifest set plus a root-level `*.f3d`.
    F3zMultiDocument,
    /// Mandatory totality row: a readable top-level manifest version this codec
    /// does not parse. Refused, so no match ever carries it.
    ///
    /// Never constructed outside the registry drift test, and that is the
    /// declared state of this row rather than an oversight: the refusal in
    /// `crate::manifest::parse_top_level` fires before a report exists, so the
    /// row is pinned without a runtime producer. Constructing it becomes
    /// possible when the refusal migrates to
    /// `CodecError::UnsupportedDialect`, which carries the match.
    #[allow(dead_code)]
    Unknown,
}

impl F3dDialect {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a variant added without a registry row, or a row
    /// added without a variant, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [Self::Manifest3200, Self::F3zMultiDocument, Self::Unknown];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::Manifest3200 => "f3d:manifest-3-2-0-0",
            Self::F3zMultiDocument => "f3d:f3z-multi-document",
            Self::Unknown => "f3d:unknown",
        })
    }

    /// Classifies a document archive from the version its top-level manifest
    /// declared.
    ///
    /// `version` is the field `crate::manifest::parse_top_level` read, not the
    /// constant it compared against. That parse admits one value and refuses
    /// every other, so reaching here means the bytes obey the row's
    /// discriminants and the admission is [`Admission::Admitted`].
    pub(crate) fn classify_document(version: &str) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(
            DECLARED_TOP_LEVEL_MANIFEST_VERSION.to_owned(),
            version.to_owned(),
        );
        Self::Manifest3200.matched(declared)
    }

    /// Classifies a multi-document F3Z archive from its root-level `*.f3d`
    /// member names, sorted by archive path.
    ///
    /// The row declares a filename-presence discriminant and no version, so a
    /// document that reaches here was read with exactly the strategy its row
    /// declares: [`Admission::Admitted`].
    pub(crate) fn classify_f3z(root_document_members: &[&str]) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(
            DECLARED_ROOT_DOCUMENT_MEMBERS.to_owned(),
            root_document_members.join(MEMBER_SEPARATOR),
        );
        Self::F3zMultiDocument.matched(declared)
    }

    /// The one [`DialectMatch`] construction path in this codec, so a
    /// classification bug and the report can never disagree.
    fn matched(self, declared: BTreeMap<String, String>) -> DialectMatch {
        DialectMatch {
            format: FORMAT.to_owned(),
            dialect: Some(self.id()),
            declared,
            admission: Admission::Admitted,
        }
    }
}

#[cfg(test)]
mod tests;
