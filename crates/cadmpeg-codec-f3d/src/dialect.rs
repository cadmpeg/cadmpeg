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
//! # Two grammars, one enum, and one recovery row
//!
//! A Fusion ZIP takes one of two document-wide parse strategies, chosen before
//! anything semantic is read (`crate::container::scan`):
//!
//! - A root `Manifest.dat` selects the binary top-level manifest grammar. The
//!   version field selects nothing: every readable version is parsed with the
//!   `3-2-0-0` layout, whose anchors (`FusionDocType`, `.f3d`, and two
//!   hyphenated GUIDs) decide whether that layout fits. A document that
//!   declares `3-2-0-0` and parses is `f3d:manifest-3-2-0-0`. A document that
//!   declares another version and still parses is `f3d:unknown`, read with a
//!   strategy its own declaration does not name.
//! - No root `Manifest.dat`, but `Manifest.json`, `DesignDescription.json`, and
//!   a root-level `*.f3d` member, selects the F3Z multi-document grammar. That
//!   branch reads no version field at all, so `f3d:f3z-multi-document` is an
//!   identity row with an unbounded interior.
//!
//! The two identity rows are [`Admission::Admitted`]: each is parsed with the
//! strategy its own row declares. [`F3dDialect::Unknown`] is the mandatory
//! totality row (design 3.3, B4) and it is
//! [`Admission::AdmittedUnverified`], naming `f3d:manifest-3-2-0-0` as the
//! strategy applied to it, with [`dialect_loss`] charging
//! `source.dialect-unverified` on exactly that admission. Refusal stays
//! structural: a manifest whose bytes do not fit the anchors is refused by
//! `crate::manifest::parse_top_level`, and no version is on an allowlist.
//!
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::codec::TargetDescriptor;
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

use crate::loss::F3dLossCode;
use crate::manifest::TOP_LEVEL_MANIFEST_VERSION;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "f3d";

/// The one dialect this writer synthesizes.
///
/// `manifest::write` pins the top-level manifest version to
/// `TOP_LEVEL_MANIFEST_VERSION`, so a generated archive can be no other row.
/// The multi-document F3Z row is reachable only by replaying a retained
/// archive, which is preservation, not synthesis.
pub(crate) const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
    id: "f3d:manifest-3-2-0-0",
    label: "Fusion 360 archive with top-level manifest 3-2-0-0",
    aliases: &["3-2-0-0"],
    default: true,
}];

/// The semantic generator's only target, structurally tied to the one-row catalog.
pub(crate) const SYNTHESIS_TARGET_ID: &str = match TARGETS {
    [target] => target.id,
    _ => panic!("the F3D synthesis catalog must contain exactly one row"),
};

/// Key of the top-level `Manifest.dat` version field in
/// [`DialectMatch::declared`], recorded as the manifest cursor read it.
///
/// The value is the length-prefixed ASCII field at the head of the manifest.
/// `crate::manifest::parse_top_level` reads it and parses on regardless, so the
/// recorded value is whatever the document declared. It is the discriminant
/// between the identity row and the recovery row, and it is what names the
/// generation the bytes came from.
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
    /// Mandatory totality row: a top-level manifest that declares a version
    /// this codec does not know, and that the `3-2-0-0` layout parsed anyway.
    ///
    /// The document is read, so the row carries a match. The strategy applied
    /// to it is the one [`Self::Manifest3200`] declares, which the document's
    /// own declaration does not name, so the admission is
    /// [`Admission::AdmittedUnverified`] and [`dialect_loss`] charges the
    /// recovery.
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
    /// constant it compared against. Reaching here means the `3-2-0-0` layout
    /// parsed the whole manifest, so the version decides only which row names
    /// that reading: its own, or the recovery row.
    pub(crate) fn classify_document(version: &str) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(
            DECLARED_TOP_LEVEL_MANIFEST_VERSION.to_owned(),
            version.to_owned(),
        );
        let dialect = if version == TOP_LEVEL_MANIFEST_VERSION {
            Self::Manifest3200
        } else {
            Self::Unknown
        };
        dialect.matched(declared)
    }

    /// How a document on this row was admitted.
    ///
    /// The one predicate behind both the report's [`Admission`] and
    /// [`dialect_loss`]: an identity row was parsed with the strategy it
    /// declares, and the recovery row was parsed with the `3-2-0-0` strategy
    /// its own declaration does not name.
    fn admission(self) -> Admission {
        match self {
            Self::Manifest3200 | Self::F3zMultiDocument => Admission::Admitted,
            Self::Unknown => Admission::AdmittedUnverified {
                nearest: Self::Manifest3200.id(),
            },
        }
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
            admission: self.admission(),
        }
    }
}

/// The dialect-unverified loss for a classified layer.
///
/// `None` exactly when `matched.admission` is [`Admission::Admitted`], because
/// this reads that field rather than reclassifying. The biconditional the
/// decode policy requires is therefore structural: the note charged and the
/// admission reported come from one value, not from two authors agreeing.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    match &matched.admission {
        Admission::Admitted | Admission::Refused => None,
        Admission::AdmittedUnverified { nearest } => {
            let version = matched
                .declared
                .get(DECLARED_TOP_LEVEL_MANIFEST_VERSION)
                .map_or("(none)", String::as_str);
            Some(F3dLossCode::SourceDialectUnverified.note(format!(
                "the top-level manifest declares version {version:?}, which no dialect row of \
                 this codec names, so no declared identity was verified. The document is read on \
                 {nearest}: every field after the version was parsed with that layout. The layout \
                 fitting is consistency, not a declaration."
            )))
        }
    }
}

#[cfg(test)]
mod tests;
