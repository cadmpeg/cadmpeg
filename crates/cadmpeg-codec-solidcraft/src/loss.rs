// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for SolidCraft design decoding.
//!
//! [`SolidcraftLossCode::note`] is the single construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code, and it leaves only the per-instance message to the
//! caller.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("solidcraft");

/// A stable, machine-readable identifier for one SolidCraft transfer loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SolidcraftLossCode {
    /// The container marker or the document format tag selects no declared
    /// dialect row.
    SourceDialectUnverified,
    /// The ZIP central directory was unreadable and members were recovered
    /// from local headers.
    ContainerDirectoryRecovered,
    /// A blob reference is not `blobs/<sha256>.<extension>` with a known
    /// extension.
    BlobReferenceMalformed,
    /// A blob reference names a member that the container does not hold.
    BlobMissing,
    /// A blob's SHA-256 differs from the digest in its name.
    BlobDigestMismatch,
    /// A blob member that no document field references.
    BlobUnreferenced,
    /// A blob payload was retained without decoding.
    PayloadUndecoded,
    /// Feature records were retained without decoding.
    HistoryUndecoded,
    /// Parameter records were retained without decoding.
    ParametersUndecoded,
    /// A top-level document section was retained without decoding.
    DocumentSectionUndecoded,
    /// The model cache was retained without decoding.
    ModelCacheUndecoded,
    /// The design carries no model cache, so it carries no evaluated geometry.
    ModelCacheAbsent,
    /// A container member outside the design document was retained without
    /// decoding.
    EntryRetained,
    /// A drawing container was retained without decoding.
    DrawingUndecoded,
    /// The document part does not parse as a JSON object.
    DocumentUnreadable,
}

impl SolidcraftLossCode {
    #[cfg(test)]
    const ALL: &'static [SolidcraftLossCode] = &[
        Self::SourceDialectUnverified,
        Self::ContainerDirectoryRecovered,
        Self::BlobReferenceMalformed,
        Self::BlobMissing,
        Self::BlobDigestMismatch,
        Self::BlobUnreferenced,
        Self::PayloadUndecoded,
        Self::HistoryUndecoded,
        Self::ParametersUndecoded,
        Self::DocumentSectionUndecoded,
        Self::ModelCacheUndecoded,
        Self::ModelCacheAbsent,
        Self::EntryRetained,
        Self::DrawingUndecoded,
        Self::DocumentUnreadable,
    ];

    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::ContainerDirectoryRecovered => "container.directory-recovered",
            Self::BlobReferenceMalformed => "container.blob-reference-malformed",
            Self::BlobMissing => "container.blob-missing",
            Self::BlobDigestMismatch => "container.blob-digest-mismatch",
            Self::BlobUnreferenced => "container.blob-unreferenced",
            Self::PayloadUndecoded => "blob.payload-undecoded",
            Self::HistoryUndecoded => "history.features-undecoded",
            Self::ParametersUndecoded => "history.parameters-undecoded",
            Self::DocumentSectionUndecoded => "document.section-undecoded",
            Self::ModelCacheUndecoded => "cache.model-undecoded",
            Self::ModelCacheAbsent => "cache.model-absent",
            Self::EntryRetained => "container.entry-retained",
            Self::DrawingUndecoded => "document.drawing-undecoded",
            Self::DocumentUnreadable => "document.unreadable",
        }
    }

    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::BlobUnreferenced | Self::DocumentSectionUndecoded | Self::EntryRetained => {
                Severity::Info
            }
            Self::SourceDialectUnverified
            | Self::ContainerDirectoryRecovered
            | Self::BlobReferenceMalformed
            | Self::BlobMissing
            | Self::BlobDigestMismatch
            | Self::PayloadUndecoded
            | Self::HistoryUndecoded
            | Self::ParametersUndecoded
            | Self::ModelCacheUndecoded
            | Self::ModelCacheAbsent
            | Self::DrawingUndecoded => Severity::Warning,
            Self::DocumentUnreadable => Severity::Error,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::ContainerDirectoryRecovered => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::BlobReferenceMalformed | Self::BlobDigestMismatch => {
                LossTaxonomy::IntegrityFailure
            }
            Self::BlobMissing | Self::BlobUnreferenced | Self::PayloadUndecoded => {
                LossTaxonomy::AssetNotTransferred
            }
            Self::HistoryUndecoded => LossTaxonomy::FeatureHistoryRetained,
            Self::ParametersUndecoded
            | Self::DocumentSectionUndecoded
            | Self::EntryRetained
            | Self::DrawingUndecoded
            | Self::DocumentUnreadable => LossTaxonomy::RecordNotTyped,
            Self::ModelCacheUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::ModelCacheAbsent => LossTaxonomy::MissingGeometryStream,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(NAMESPACE, self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::SolidcraftLossCode;

    #[test]
    fn code_strings_are_pinned() {
        assert_eq!(
            SolidcraftLossCode::ALL
                .iter()
                .map(|code| code.code())
                .collect::<Vec<_>>(),
            [
                "source.dialect-unverified",
                "container.directory-recovered",
                "container.blob-reference-malformed",
                "container.blob-missing",
                "container.blob-digest-mismatch",
                "container.blob-unreferenced",
                "blob.payload-undecoded",
                "history.features-undecoded",
                "history.parameters-undecoded",
                "document.section-undecoded",
                "cache.model-undecoded",
                "cache.model-absent",
                "container.entry-retained",
                "document.drawing-undecoded",
                "document.unreadable",
            ]
        );
    }

    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in SolidcraftLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    #[test]
    fn note_takes_severity_from_the_code() {
        for code in SolidcraftLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
        }
    }
}
