// SPDX-License-Identifier: Apache-2.0
//! NX dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`NxDialect::classify`] is the one construction
//! path, and the vocabulary is closed. Tests close it directly against the
//! reportable rows in `docs/dialects.toml`.
//!
//! # Host classification is structural
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
//! Each of the two host rows therefore names the grammar that actually parsed
//! the document, so admission is [`Admission::Admitted`] on both. Embedded
//! Parasolid streams form separate layers. A schema without a named grammar is
//! admitted as residual and charges `source.kernel-dialect-unverified`; host
//! admission does not launder the kernel layer.
//!
//! # The version byte is provenance, not a discriminant
//!
//! `Container::version` is a `u8` — file offset 8 on the modern arm, byte 8 of
//! the UGII payload prefix on the legacy arm. No branch in this codec reads it.
//! Its consumers are report notes and [`DialectMatch::declared`]. It is
//! recorded verbatim under the key the arm that read it already used, and it
//! never moves the resolved id.
//!
//! A successful scan always reads the version byte, so the value used by decode
//! and the declaration recorded here cannot diverge.
//!
//! # `nx:unknown` is a registry-only row this codec never emits
//!
//! The registry's mandatory totality row covers a part file
//! matching neither container. NX refuses such a file at the container
//! boundary — `NxCodec::detect` reports
//! [`cadmpeg_ir::codec::Confidence::No`], and a forced scan returns
//! `WrongFormat` — so no run ever produces a report to carry the row. The
//! `unknown_kind = "detect-unreachable"` excludes it from reportable-row
//! closure. No codec enum variant or report construction path names it.

use crate::container::Container;
use crate::loss::NxLossCode;
use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_ir::LossNote;

use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "nx";
#[cfg(test)]
const PARASOLID_FORMAT: &str = "parasolid";

/// Key of the modern container version byte in [`DialectMatch::declared`].
///
/// A successful modern scan always reads this byte from the source.
const DECLARED_SPLMSSTR_VERSION: &str = "splmsstr_version";
/// Key of the first object-model store's version in [`DialectMatch::declared`].
///
/// Absent when no indexed store section carries a `store_version` record.
const DECLARED_PRODUCT_VERSION: &str = "product_version";
/// Key of the legacy UGII payload version byte in [`DialectMatch::declared`].
const DECLARED_UGII_VERSION: &str = "ugii_version";

/// One reportable row of `docs/dialects.toml` under the `nx` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NxDialect {
    /// The `SPLMSSTR` member table.
    Splmsstr,
    /// The legacy Compound File envelope around a `UG_PART/UG_PART` stream.
    LegacyCfb,
}

/// Classify the host container and every schema-bearing Parasolid stream.
pub(crate) fn classify_layers(scan: &crate::decode::Scan<'_>) -> DialectLayers {
    let streams = scan
        .streams
        .iter()
        .filter_map(|stream| {
            stream
                .kind
                .is_parasolid()
                .then_some(stream)
                .and_then(|stream| stream.schema.as_deref().map(|schema| (stream, schema)))
        })
        .collect::<Vec<_>>();
    let extra = cadmpeg_parasolid::extra_layers(
        streams
            .into_iter()
            .map(|(stream, schema)| (schema.to_owned(), format!("stream@{}", stream.file_offset)))
            .collect(),
    );
    DialectLayers::new(NxDialect::classify(&scan.container), extra)
        .expect("Parasolid stream offsets have unique instances")
}

/// Losses charged by every unverified layer in a classified document.
///
/// This walks the complete layer set rather than assuming the host primary is
/// the only identity that can affect decode policy.
pub(crate) fn dialect_losses(layers: &DialectLayers) -> Vec<LossNote> {
    layers
        .iter()
        .filter_map(cadmpeg_parasolid::unverified_message)
        .map(|message| NxLossCode::KernelDialectUnverified.note(message))
        .collect()
}

impl NxDialect {
    /// Every reportable dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::Splmsstr, Self::LegacyCfb];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::Splmsstr => "nx:splmsstr",
            Self::LegacyCfb => "nx:legacy-cfb",
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

    /// Classifies one document. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification bug and the report
    /// can never disagree.
    ///
    /// The declared keys are the version fields the arm that ran actually read.
    /// They are evidence: the resolved id comes from the container dispatch and
    /// never from them.
    pub(crate) fn classify(container: &Container<'_>) -> DialectMatch {
        let dialect = Self::of_container(container);
        let mut declared = BTreeMap::new();
        match dialect {
            Self::LegacyCfb => {
                declared.insert(DECLARED_UGII_VERSION.into(), container.version.to_string());
            }
            Self::Splmsstr => {
                declared.insert(
                    DECLARED_SPLMSSTR_VERSION.into(),
                    container.version.to_string(),
                );
                if let Some(header) = crate::native::store_headers(container).first() {
                    declared.insert(DECLARED_PRODUCT_VERSION.into(), header.version.clone());
                }
            }
        }
        DialectMatch::admitted(dialect.id()).with_declared(declared)
    }
}

#[cfg(test)]
mod tests;
