// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.FCStd` decoding.
//!
//! Every native-retention loss the decoder reports carries a stable
//! machine-readable code from [`FcstdLossCode`]. The code fixes the loss
//! category, severity, and shared [`LossCode`] so the four report sites cannot
//! drift apart on how a given loss is classified, and a new drop path without a
//! code does not compile.
//!
//! [`FcstdLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate. It stamps the fixed `fcstd`/`Document.xml`
//! provenance shape shared by every site and leaves only the per-instance
//! message and the retained record's native reference to the caller.
use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};
use cadmpeg_ir::LossProvenance;

/// A stable, machine-readable identifier for one `.FCStd` transfer loss.
///
/// The string form (via [`FcstdLossCode::code`]) is the stable contract; the
/// Rust variant name may be refactored freely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FcstdLossCode {
    /// Native design operation is retained but has no neutral semantics.
    DesignOperationNativeRetained,
    /// Linear-pattern direction is retained as a native reference, unresolved.
    LinearPatternDirectionUnresolved,
    /// Native sketch geometry is retained without a neutralized form.
    SketchGeometryNativeRetained,
    /// Native sketch constraint is retained without a neutralized form.
    SketchConstraintNativeRetained,
}

impl FcstdLossCode {
    /// Every code, in declaration order. Used by tests to assert stability.
    pub const ALL: &'static [FcstdLossCode] = &[
        Self::DesignOperationNativeRetained,
        Self::LinearPatternDirectionUnresolved,
        Self::SketchGeometryNativeRetained,
        Self::SketchConstraintNativeRetained,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::DesignOperationNativeRetained => "feature.design-operation-native-retained",
            Self::LinearPatternDirectionUnresolved => "feature.linear-pattern-direction-unresolved",
            Self::SketchGeometryNativeRetained => "sketch.native-geometry-retained",
            Self::SketchConstraintNativeRetained => "sketch.native-constraint-retained",
        }
    }

    /// The subsystem category this loss belongs to.
    #[must_use]
    pub const fn category(self) -> LossCategory {
        match self {
            Self::SketchGeometryNativeRetained => LossCategory::Geometry,
            Self::DesignOperationNativeRetained
            | Self::LinearPatternDirectionUnresolved
            | Self::SketchConstraintNativeRetained => LossCategory::Other,
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        // Every FCStd native-retention loss blocks lossless reconstruction.
        Severity::Blocking
    }

    /// The shared IR loss code this loss maps onto.
    #[must_use]
    pub const fn shared_code(self) -> LossCode {
        match self {
            Self::DesignOperationNativeRetained => LossCode::FeatureHistoryRetained,
            Self::LinearPatternDirectionUnresolved => LossCode::ParametricRecordOmitted,
            Self::SketchGeometryNativeRetained | Self::SketchConstraintNativeRetained => {
                LossCode::RecordNotTyped
            }
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message
    /// and the retained record's native reference.
    ///
    /// Category, severity, and the shared code come from the loss code, so a
    /// site cannot mislabel a loss it names. Provenance is the fixed
    /// `fcstd`/`Document.xml` shape every site emits, carrying `native_ref` as
    /// the record tag.
    #[must_use]
    pub fn note(self, message: impl Into<String>, native_ref: Option<String>) -> LossNote {
        LossNote {
            code: self.shared_code(),
            category: self.category(),
            severity: self.severity(),
            message: message.into(),
            provenance: Some(LossProvenance {
                format: "fcstd".into(),
                stream: "Document.xml".into(),
                offset: 0,
                tag: native_ref,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FcstdLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned. A diff
    /// here is an intentional contract change to a gating identifier.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = FcstdLossCode::ALL.iter().map(|code| code.code()).collect();
        assert_eq!(
            codes,
            [
                "feature.design-operation-native-retained",
                "feature.linear-pattern-direction-unresolved",
                "sketch.native-geometry-retained",
                "sketch.native-constraint-retained",
            ]
        );
    }

    /// Codes are unique and lower-kebab within dotted segments.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in FcstdLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            assert!(text.contains('.'), "code {text} has no domain segment");
            assert!(
                text.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-'),
                "code {text} is not lower-kebab"
            );
        }
    }

    /// A note inherits its category, severity, and shared code from the loss
    /// code, and stamps the fixed FCStd provenance shape.
    #[test]
    fn note_takes_classification_from_the_code() {
        for code in FcstdLossCode::ALL {
            let note = code.note("message", Some("tag".into()));
            assert_eq!(note.category, code.category());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.code, code.shared_code());
            let provenance = note.provenance.expect("fcstd provenance");
            assert_eq!(provenance.format, "fcstd");
            assert_eq!(provenance.stream, "Document.xml");
            assert_eq!(provenance.offset, 0);
            assert_eq!(provenance.tag.as_deref(), Some("tag"));
        }
    }
}
