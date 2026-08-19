// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `FreeCAD` `.fcstd` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a
//! stable machine-readable code from [`FreecadLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`FreecadLossCode::note`] is the single practical construction path for a
//! [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller.
//!
use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

/// A stable, machine-readable identifier for one `.fcstd` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`FreecadLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FreecadLossCode {
    /// Feature history cannot enter a neutral definition because its ordering is cyclic.
    FeatureCyclicHistory,
    /// Feature retains its native kind without a complete neutral operation.
    FeatureNativeKindRetained,
    /// Sketch geometry record retains a native kind without solved geometry.
    SketchNativeGeometry,
    /// Sketch constraint retains a native relation kind without neutral semantics.
    SketchNativeConstraint,
    /// Topology color values were retained because their count did not match mapped topology.
    AppearanceTopologyColorCountMismatch,
}

impl FreecadLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    pub const ALL: &'static [FreecadLossCode] = &[
        Self::FeatureCyclicHistory,
        Self::FeatureNativeKindRetained,
        Self::SketchNativeGeometry,
        Self::SketchNativeConstraint,
        Self::AppearanceTopologyColorCountMismatch,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::FeatureCyclicHistory => "feature.cyclic-history",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::SketchNativeGeometry => "sketch.native-geometry",
            Self::SketchNativeConstraint => "sketch.native-constraint",
            Self::AppearanceTopologyColorCountMismatch => {
                "appearance.topology-color-count-mismatch"
            }
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::FeatureCyclicHistory
            | Self::FeatureNativeKindRetained
            | Self::SketchNativeGeometry
            | Self::SketchNativeConstraint => Severity::Blocking,
            Self::AppearanceTopologyColorCountMismatch => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::FeatureCyclicHistory | Self::FeatureNativeKindRetained => {
                LossTaxonomy::FeatureHistoryRetained
            }
            Self::SketchNativeGeometry | Self::SketchNativeConstraint => {
                LossTaxonomy::RecordNotTyped
            }
            Self::AppearanceTopologyColorCountMismatch => LossTaxonomy::MaterialNotTransferred,
        }
    }

    /// Strict floor pinned from this local code (independent of taxonomy remap).
    ///
    /// Defaults to the taxonomy floor so a later local→taxonomy remap cannot
    /// silently change rejection; list only intentional overrides here.
    const fn strict_floor(self) -> Option<Severity> {
        self.shared_taxonomy().strict_floor()
    }

    /// Namespaced [`LossKind`] for this local code (taxonomy + pinned floor).
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced("fcstd", self.code(), self.shared_taxonomy())
            .with_strict_floor(self.strict_floor())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `fcstd/<local>`. Severity and strict floor come
    /// from the local code.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::FreecadLossCode;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = FreecadLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "feature.cyclic-history",
                "feature.native-kind-retained",
                "sketch.native-geometry",
                "sketch.native-constraint",
                "appearance.topology-color-count-mismatch",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in FreecadLossCode::ALL {
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
        for code in FreecadLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
