// SPDX-License-Identifier: Apache-2.0
//! NX dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`NxDialect::classify`] is the one construction
//! path, and the vocabulary is closed. Every variant here has a row in
//! `docs/dialects.toml`; the registry cross-check in this module's tests fails
//! on drift in either direction.
//!
//! # Classification is structural, so every admitted document is `Admitted`
//!
//! NX has two container grammars and one dispatch, at [`crate::decode::scan`]:
//! a file whose bytes start with `SPLMSSTR` is parsed by
//! [`crate::container::scan_bytes`], and anything else is handed to
//! [`crate::container::scan_legacy`], which requires a Compound File envelope
//! carrying `UG_PART/UG_PART` and a `\x0d\x01UGII  ` payload prefix and errors
//! with [`cadmpeg_core::CodecError::WrongFormat`] otherwise. The parser that
//! ran is recorded on `Container::legacy_cfb`, and that flag — not a second
//! reading of the magic — is what this module classifies on.
//!
//! Each of the two rows therefore names the grammar that actually parsed the
//! document, so admission is [`Admission::Admitted`] on both. There is no
//! unverified path to invent: the container version byte is never compared to
//! anything, and this codec charges no dialect-unverified loss.
//!
//! # The version byte is provenance, not a discriminant
//!
//! `Container::version` is a `u8` — file offset 8 on the modern arm, byte 8 of
//! the UGII payload prefix on the legacy arm. No branch in this codec reads it.
//! Its consumers are report notes and the source attributes, which is exactly
//! the role [`DialectMatch::declared`] exists for. It is recorded verbatim
//! under the key the arm that read it already used, and it never moves the
//! resolved id.
//!
//! A successful scan always reads the version byte, so the value used by decode
//! and the declaration recorded here cannot diverge.
//!
//! # `nx:unknown` is a declared row this codec never emits
//!
//! The registry's mandatory totality row covers a part file
//! matching neither container. NX refuses such a file at the container
//! boundary — `NxCodec::detect` reports
//! [`cadmpeg_ir::codec::Confidence::No`], and a forced scan returns
//! `WrongFormat` — so no run ever produces a report to carry the row. The
//! variant exists here for the registry cross-check, and the tests pin that it
//! stays unreachable from [`NxDialect::classify`].

use crate::container::Container;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "nx";

/// Key of the modern container version byte in [`DialectMatch::declared`].
///
/// A successful modern scan always reads this byte from the source.
const DECLARED_SPLMSSTR_VERSION: &str = "splmsstr_version";
/// Key of the first object-model store's version in [`DialectMatch::declared`].
///
/// Absent when no indexed store section carries a `store_version` record.
const DECLARED_PRODUCT_VERSION: &str = "product_version";
/// Key of the legacy UGII payload version byte in [`DialectMatch::declared`].
///
const DECLARED_UGII_VERSION: &str = "ugii_version";

/// One row of `docs/dialects.toml` under the `nx` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NxDialect {
    /// The `SPLMSSTR` member table.
    Splmsstr,
    /// The legacy Compound File envelope around a `UG_PART/UG_PART` stream.
    LegacyCfb,
    /// Mandatory totality row; refused at the container boundary, never
    /// classified.
    // Constructed only by the registry cross-check. A file matching neither
    // container never reaches a report, so production code names this row and
    // never builds one; deleting the variant would delete the registry row's
    // only in-code anchor.
    #[allow(dead_code)]
    Unknown,
}

impl NxDialect {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a variant added without a registry row, or a row
    /// added without a variant, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [Self::Splmsstr, Self::LegacyCfb, Self::Unknown];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::Splmsstr => "nx:splmsstr",
            Self::LegacyCfb => "nx:legacy-cfb",
            Self::Unknown => "nx:unknown",
        })
    }

    /// The `ContainerSummary::container_kind` label for this row.
    ///
    /// One source for the label and the id, so a summary cannot name a
    /// container the classification disagrees with.
    pub(crate) const fn container_kind(self) -> &'static str {
        match self {
            Self::Splmsstr => "splmsstr",
            Self::LegacyCfb => "cfb",
            Self::Unknown => "unknown",
        }
    }

    /// The row for a parsed container.
    ///
    /// The predicate is the dispatch itself: `Container::is_legacy_cfb` is set
    /// by whichever of the two parsers built the container, so this reads the
    /// decision rather than repeating it.
    pub(crate) fn of_container(container: &Container<'_>) -> Self {
        if container.is_legacy_cfb() {
            Self::LegacyCfb
        } else {
            Self::Splmsstr
        }
    }

    /// How a document on this row was admitted.
    ///
    /// The one admission predicate in this codec. Both container rows name the
    /// grammar that parsed the document, so both are admitted; nothing in NX
    /// substitutes one row's strategy for another's, and no dialect-unverified
    /// loss exists to charge. [`Self::Unknown`] is unreachable from
    /// [`Self::of_container`] and carries the refusal disposition the registry
    /// records; it answers here only to keep the function total.
    pub(crate) const fn admission(self) -> Admission {
        match self {
            Self::Splmsstr | Self::LegacyCfb => Admission::Admitted,
            Self::Unknown => Admission::Refused,
        }
    }

    /// Classifies one document. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification bug and the report
    /// can never disagree.
    ///
    /// The declared keys are the version fields the arm that ran actually read,
    /// under the keys the source attributes already use. They are evidence: the
    /// resolved id comes from the container dispatch and never from them.
    pub(crate) fn classify(container: &Container<'_>) -> DialectMatch {
        let dialect = Self::of_container(container);
        let mut declared = BTreeMap::new();
        match dialect {
            Self::LegacyCfb => {
                declared.insert(DECLARED_UGII_VERSION.into(), container.version.to_string());
            }
            // `of_container` returns one of the two container rows and never
            // `Unknown`, which shares the modern arm only to keep the match
            // total.
            Self::Splmsstr | Self::Unknown => {
                declared.insert(
                    DECLARED_SPLMSSTR_VERSION.into(),
                    container.version.to_string(),
                );
                if let Some(header) = crate::native::store_headers(container).first() {
                    declared.insert(DECLARED_PRODUCT_VERSION.into(), header.version.clone());
                }
            }
        }
        DialectMatch::layer(FORMAT, dialect.id(), declared, dialect.admission())
    }
}

#[cfg(test)]
mod tests;
