// SPDX-License-Identifier: Apache-2.0
//! Inventor dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, `DialectId::pinned` strings
//! are the boundary, [`DialectRecovery::dialect_match`] is the one
//! construction path, and the vocabulary is closed. Every variant here has a row in
//! `docs/dialects.toml`; `tests::every_pinned_id_has_a_registry_row` fails on
//! drift in either direction. Two variants is still a closed vocabulary, and
//! the drift test is what keeps it one.
//!
//! # Identity and admission coincide here, and that is a statement
//!
//! IGES separates the two because it declares identity rows for versions whose
//! grammar it never verified. Inventor declares two rows, and their
//! discriminants *are* the codec's two version gates: the `RSeDb` schema and
//! the `RSe` Meta Stream marker and version. So a document that satisfies
//! `inventor:cfb3-rse31-meta8` is exactly a document read with the grammar that
//! row declares, and everything else is `inventor:unknown` read with a grammar
//! no row declares for it. One predicate — [`DialectRecovery::is_verified`] —
//! therefore decides both, and [`DialectRecovery::dialect_loss`] is `None`
//! exactly when it holds. The biconditional the decode policy requires is
//! structural, not maintained by two authors agreeing.
//!
//! # The row absorbs what the codec does not gate
//!
//! Neither gate refuses. A schema other than 31 leaves the `RSeDb` stream and
//! the segment registry unavailable, and a Meta Stream other than version 8
//! leaves that segment's metadata unread; decode continues in both cases and
//! degrades. That is [`Admission::AdmittedUnverified`] exactly, and `nearest`
//! names `inventor:cfb3-rse31-meta8` because the schema-31 registry grammar and
//! the version-8 metadata grammar are the only ones this codec implements —
//! they are the strategy it applied, in the parts it could apply.
//!
//! The pinned id says `cfb3`, and the codec never tests the CFB major version:
//! the shared compound parser accepts major 3 and major 4, and neither row
//! carries a `cfb_major_version` discriminant. A CFB v4 Inventor document
//! therefore classifies as `inventor:cfb3-rse31-meta8` when its `RSe`
//! declarations are the verified ones. Ids are pinned forever, so the name
//! stays and the fact is written down here and in the registry rather than
//! silently corrected. The observed major version is reported under
//! [`DialectMatch::declared`], which is where a declaration the codec does not
//! branch on belongs.
//!
//! # Absence of a declaration is not verification
//!
//! A document with no `RSeDb` stream declares no schema, and a document with no
//! readable Meta Stream declaration declares no metadata version. Neither
//! satisfies a discriminant of `inventor:cfb3-rse31-meta8`, so both land on the
//! totality row. This is not a technicality: with no `RSeDb` schema the
//! decoder already refuses to apply the registry grammar
//! (`rse.rs`, "`RSe` database schemas do not select one registry grammar"), so
//! reporting the document as the verified dialect would contradict what the
//! decode did.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;

use crate::container::InventorContainer;
use crate::database::RseSchema;
use crate::loss::InventorLossCode;
use crate::rse::MetaStreamDeclaration;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "inventor";

/// Key of the CFB major version in [`DialectMatch::declared`].
///
/// Evidence the codec reports and never branches on: the shared compound
/// parser admits major 3 and major 4 alike.
const DECLARED_CFB_MAJOR_VERSION: &str = "cfb_major_version";
/// Key of the `RSeDb` schema declarations in [`DialectMatch::declared`].
///
/// A document may carry several `V<n>/RSeDb` streams. The value is every
/// distinct schema they declared, ascending, separated by `,`. The key is
/// absent when no `RSeDb` stream read as far as its schema word.
const DECLARED_RSE_DB_SCHEMA: &str = "rse_db_schema";
/// Key of the `RSe` Meta Stream marker declarations in [`DialectMatch::declared`].
///
/// Every distinct marker the segment metadata streams declared, in ascending
/// order, separated by `,`. Absent when no metadata stream read as far as its
/// marker.
const DECLARED_META_STREAM_MARKER: &str = "meta_stream_marker";
/// Key of the `RSe` Meta Stream version declarations in [`DialectMatch::declared`].
///
/// Every distinct version word the segment metadata streams declared,
/// ascending, separated by `,`. Absent under the same condition as
/// [`DECLARED_META_STREAM_MARKER`].
const DECLARED_META_STREAM_VERSION: &str = "meta_stream_version";

/// Joins declaration values into one `declared` entry.
fn join(values: impl IntoIterator<Item = String>) -> String {
    values.into_iter().collect::<Vec<_>>().join(",")
}

/// One row of `docs/dialects.toml` under the `inventor` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InventorDialect {
    /// `RSeDb` schema 31 and `RSe` Meta Stream version 8, both declared.
    Cfb3Rse31Meta8,
    /// The mandatory totality row (design §3.3, B4): any other declaration, and
    /// the absence of one. Admitted and degraded, never refused.
    Unknown,
}

impl InventorDialect {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a variant added without a registry row, or a row
    /// added without a variant, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::Cfb3Rse31Meta8, Self::Unknown];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::Cfb3Rse31Meta8 => "inventor:cfb3-rse31-meta8",
            Self::Unknown => "inventor:unknown",
        })
    }
}

/// The version declarations one document carries, read where the decoder reads
/// them.
///
/// Built once from the parsed container and consulted by both the admission and
/// the loss. Nothing here re-reads bytes: the `RSeDb` schema survives its own
/// rejection on [`crate::database::DatabaseHeader`], and the Meta Stream marker
/// and version survive theirs on [`crate::rse::SegmentMetaState`].
pub(crate) struct DialectRecovery {
    /// CFB major version, as the compound header declared it.
    cfb_major_version: u16,
    /// Distinct `RSeDb` schema declarations, ascending.
    schemas: Vec<RseSchema>,
    /// Distinct `RSe` Meta Stream declarations, ascending.
    meta_streams: Vec<MetaStreamDeclaration>,
}

impl DialectRecovery {
    /// Collects every version declaration the decode read from `container`.
    pub(crate) fn of(container: &InventorContainer<'_>) -> Self {
        let mut schemas = container
            .rse
            .databases
            .iter()
            .filter_map(|descriptor| descriptor.declared_schema)
            .collect::<Vec<_>>();
        schemas.sort_unstable_by_key(|schema| schema.value());
        schemas.dedup();
        let mut meta_streams = container
            .rse
            .segments
            .iter()
            .filter_map(|segment| segment.meta.declaration())
            .collect::<Vec<_>>();
        meta_streams.sort();
        meta_streams.dedup();
        Self {
            cfb_major_version: container.snapshot.major_version(),
            schemas,
            meta_streams,
        }
    }

    /// Whether every declaration this document carries selects a grammar this
    /// codec implements, and it carries one of each.
    ///
    /// The single predicate behind two facts that must never disagree: the
    /// [`InventorLossCode::SourceDialectUnverified`] charge in
    /// [`Self::dialect_loss`] and the [`Admission`] in [`Self::dialect_match`].
    /// Both call this; neither recomputes it.
    pub(crate) fn is_verified(&self) -> bool {
        !self.schemas.is_empty()
            && self
                .schemas
                .iter()
                .all(|schema| *schema == RseSchema::SCHEMA_31)
            && !self.meta_streams.is_empty()
            && self
                .meta_streams
                .iter()
                .all(MetaStreamDeclaration::is_verified)
    }

    /// The row whose discriminants this document satisfies.
    fn dialect(&self) -> InventorDialect {
        if self.is_verified() {
            InventorDialect::Cfb3Rse31Meta8
        } else {
            InventorDialect::Unknown
        }
    }

    /// This document's [`DialectMatch`], identity and admission together.
    pub(crate) fn dialect_match(&self) -> DialectMatch {
        let admission = if self.is_verified() {
            Admission::Admitted
        } else {
            Admission::AdmittedUnverified {
                nearest: InventorDialect::Cfb3Rse31Meta8.id(),
            }
        };
        let mut declared = BTreeMap::new();
        declared.insert(
            DECLARED_CFB_MAJOR_VERSION.into(),
            self.cfb_major_version.to_string(),
        );
        if !self.schemas.is_empty() {
            declared.insert(
                DECLARED_RSE_DB_SCHEMA.into(),
                join(self.schemas.iter().map(|schema| schema.value().to_string())),
            );
        }
        if !self.meta_streams.is_empty() {
            declared.insert(
                DECLARED_META_STREAM_MARKER.into(),
                join(
                    self.meta_streams
                        .iter()
                        .map(|declared| declared.marker.clone()),
                ),
            );
            declared.insert(
                DECLARED_META_STREAM_VERSION.into(),
                join(
                    self.meta_streams
                        .iter()
                        .map(|declared| declared.version.to_string()),
                ),
            );
        }
        DialectMatch {
            format: FORMAT.into(),
            dialect: Some(self.dialect().id()),
            declared,
            admission,
        }
    }

    /// The loss charged when the document's declarations do not select the
    /// grammar this codec read it with.
    ///
    /// `None` exactly when [`Self::is_verified`] holds, which is also exactly
    /// when [`Self::dialect_match`] reports [`Admission::Admitted`].
    pub(crate) fn dialect_loss(&self) -> Option<LossNote> {
        if self.is_verified() {
            return None;
        }
        let mut reasons = Vec::new();
        if self.schemas.is_empty() {
            reasons.push("no RSe database stream declares a schema".to_owned());
        } else if self
            .schemas
            .iter()
            .any(|schema| *schema != RseSchema::SCHEMA_31)
        {
            reasons.push(format!(
                "RSe database schema {} is declared",
                join(self.schemas.iter().map(|schema| schema.value().to_string()))
            ));
        }
        if self.meta_streams.is_empty() {
            reasons.push("no RSe segment metadata stream declares a marker and version".to_owned());
        } else if self
            .meta_streams
            .iter()
            .any(|declared| !declared.is_verified())
        {
            reasons.push(format!(
                "RSe segment metadata marker {} version {} is declared",
                join(
                    self.meta_streams
                        .iter()
                        .map(|declared| format!("{:?}", declared.marker))
                ),
                join(
                    self.meta_streams
                        .iter()
                        .map(|declared| declared.version.to_string())
                )
            ));
        }
        Some(InventorLossCode::SourceDialectUnverified.note(format!(
            "{}; this decode read the document with the only Inventor grammars this codec \
             implements: RSe database schema {} and RSe segment metadata marker {:?} version {}",
            reasons.join("; "),
            RseSchema::SCHEMA_31.value(),
            MetaStreamDeclaration::VERIFIED_MARKER,
            MetaStreamDeclaration::VERIFIED_VERSION
        )))
    }
}

#[cfg(test)]
mod tests;
