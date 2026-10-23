// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.pvz` package decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`PvzLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text.
//!
//! [`PvzLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the loss category and severity from
//! the code, and it leaves only the per-instance message to the caller.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("pvz");

/// A stable, machine-readable identifier for one `.pvz` transfer loss.
///
/// The string form (via [`PvzLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PvzLossCode {
    /// The structure and geometry members were located but not decoded.
    PackageContentUndecoded,
    /// The structure member carries no signature of a declared dialect row.
    SourceDialectUnverified,
    /// The ZIP central directory was unreadable and members were recovered
    /// from local headers.
    ContainerDirectoryRecovered,
}

impl PvzLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [PvzLossCode] = &[
        Self::PackageContentUndecoded,
        Self::SourceDialectUnverified,
        Self::ContainerDirectoryRecovered,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::PackageContentUndecoded => "package.content-undecoded",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::ContainerDirectoryRecovered => "container.directory-recovered",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::PackageContentUndecoded => Severity::Blocking,
            Self::SourceDialectUnverified | Self::ContainerDirectoryRecovered => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::PackageContentUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::ContainerDirectoryRecovered => LossTaxonomy::NoncanonicalSourceSyntax,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(NAMESPACE, self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `pvz/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::PvzLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = PvzLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "package.content-undecoded",
                "source.dialect-unverified",
                "container.directory-recovered",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in PvzLossCode::ALL {
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

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in PvzLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
