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
use cadmpeg_ir::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("fcstd");

/// A stable, machine-readable identifier for one `.fcstd` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`FreecadLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FreecadLossCode {
    /// Template geometry or positioned text remains in its SVG asset.
    TemplateNotTransferred,
    /// The stored horizontal view axis has no nonzero neutral direction.
    DrawingViewAxisNotTransferred,
    /// Projected model-view edges have no persisted curve collection.
    ViewGeometryNotStored,
    /// The view family has no admitted projected geometry.
    ViewGeometryNotTransferred,
    /// An exact dimension value has no admitted page reference anchors.
    DimensionAnchorsNotTransferred,
    /// Drawing appearance has no complete physical style.
    DrawingStyleNotTransferred,
    /// Caption text has no normalized page position.
    CaptionNotTransferred,
    /// Persisted drawing content remains native.
    DrawingSheetNotTransferred,
    /// Persisted drawing content remains native.
    DrawingTitleBlockNotTransferred,
    /// Persisted drawing content remains native.
    DrawingCosmeticEdgeNotTransferred,
    /// Persisted drawing content remains native.
    DrawingCosmeticVertexNotTransferred,
    /// Persisted drawing content remains native.
    DrawingCenterLineNotTransferred,
    /// Persisted drawing content remains native.
    DrawingGeometryFormatNotTransferred,
    /// Persisted drawing content remains native.
    DrawingDimensionKindNotTransferred,
    /// Persisted drawing content remains native.
    DrawingDimensionGeometryNotTransferred,
    /// Persisted drawing content remains native.
    DrawingDimensionToleranceNotTransferred,
    /// A bounded archive payload cannot be opened; its stored bytes remain retained.
    ArchiveEntryUnreadable,
    /// A mesh use has no complete neutral placement path.
    MeshPlacementNotTransferred,
    /// Mesh occurrence expansion reached its input-scaled local work cap.
    MeshPlacementBounded,
    /// A property payload cannot enter the admitted persistence graph.
    PersistencePayloadUnresolved,
    /// An optional file reference has no readable archive payload.
    PersistenceSideEntryUnresolved,
    /// Redundant persistence counts disagree with framed XML populations.
    PersistenceCountNoncanonical,
    /// Feature history cannot enter a neutral definition because its ordering is cyclic.
    FeatureCyclicHistory,
    /// Feature retains its native kind without a complete neutral operation.
    FeatureNativeKindRetained,
    /// A revolution has no complete admitted angular construction.
    FeatureRevolutionNotTransferred,
    /// Sketch geometry record retains a native kind without solved geometry.
    SketchNativeGeometry,
    /// Cached sketch geometry has no matching live external reference.
    SketchExternalReferenceUnresolved,
    /// Optional external sketch cache cannot be projected; live links remain authoritative.
    SketchExternalCacheUnresolved,
    /// Sketch constraint retains a native relation kind without neutral semantics.
    SketchNativeConstraint,
    /// Sketch profiles are not chained because endpoint matching reached its work cap.
    SketchProfileMatchingBounded,
    /// Topology color values were retained because their count did not match mapped topology.
    AppearanceTopologyColorCountMismatch,
    /// A primitive style size could not enter the neutral appearance.
    AppearancePrimitiveSizeNotTransferred,
    /// The declared persistence schema names no dialect this codec has a strategy for.
    SourceDialectUnverified,
    /// The GUI document used schema-1 vocabulary under another declaration.
    SourceGuiSchemaUnverified,
    /// A GUI property states a blank key, so its value has no name to carry.
    SourceGuiPropertyKeyBlank,
    /// Optional GUI records could not be interpreted; exact XML remains retained.
    SourceGuiMetadataUnresolved,
    /// Optional persistent naming metadata could not be interpreted.
    ElementMapMetadataUnresolved,
    /// A parameter-space curve could not enter neutral geometry.
    PcurveNotTransferred,
    /// Equal-matrix pcurves have no admitted matching native location chain.
    PcurveLocationAmbiguous,
    /// Native topology remains retained when location identity expansion reaches its local cap.
    TopologyLocationIdentityBounded,
    /// Producer version metadata used an alias or conflicting declarations.
    ProgramVersionNoncanonical,
    /// Persistence version metadata used a lowercase alias.
    FileVersionNoncanonical,
    /// Persistence file-version declaration cannot select a legacy map encoding.
    FileVersionUnverified,
    /// Nonempty inline legacy map has no verified file-version interpretation.
    ElementMapVersionUnresolved,
}

impl FreecadLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [FreecadLossCode] = &[
        Self::TemplateNotTransferred,
        Self::DrawingViewAxisNotTransferred,
        Self::ViewGeometryNotStored,
        Self::ViewGeometryNotTransferred,
        Self::DimensionAnchorsNotTransferred,
        Self::DrawingStyleNotTransferred,
        Self::CaptionNotTransferred,
        Self::DrawingSheetNotTransferred,
        Self::DrawingTitleBlockNotTransferred,
        Self::DrawingCosmeticEdgeNotTransferred,
        Self::DrawingCosmeticVertexNotTransferred,
        Self::DrawingCenterLineNotTransferred,
        Self::DrawingGeometryFormatNotTransferred,
        Self::DrawingDimensionKindNotTransferred,
        Self::DrawingDimensionGeometryNotTransferred,
        Self::DrawingDimensionToleranceNotTransferred,
        Self::ArchiveEntryUnreadable,
        Self::MeshPlacementNotTransferred,
        Self::MeshPlacementBounded,
        Self::PersistencePayloadUnresolved,
        Self::PersistenceSideEntryUnresolved,
        Self::PersistenceCountNoncanonical,
        Self::FeatureCyclicHistory,
        Self::FeatureNativeKindRetained,
        Self::FeatureRevolutionNotTransferred,
        Self::SketchNativeGeometry,
        Self::SketchNativeConstraint,
        Self::SketchExternalReferenceUnresolved,
        Self::SketchExternalCacheUnresolved,
        Self::SketchProfileMatchingBounded,
        Self::AppearanceTopologyColorCountMismatch,
        Self::AppearancePrimitiveSizeNotTransferred,
        Self::SourceDialectUnverified,
        Self::SourceGuiSchemaUnverified,
        Self::SourceGuiPropertyKeyBlank,
        Self::SourceGuiMetadataUnresolved,
        Self::ElementMapMetadataUnresolved,
        Self::PcurveNotTransferred,
        Self::PcurveLocationAmbiguous,
        Self::TopologyLocationIdentityBounded,
        Self::ProgramVersionNoncanonical,
        Self::FileVersionNoncanonical,
        Self::FileVersionUnverified,
        Self::ElementMapVersionUnresolved,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::DrawingViewAxisNotTransferred => "drawing.view-axis-not-transferred",
            Self::TemplateNotTransferred => "drawing.template-not-transferred",
            Self::ViewGeometryNotStored => "drawing.view-geometry-not-stored",
            Self::ViewGeometryNotTransferred => "drawing.view-geometry-not-transferred",
            Self::DimensionAnchorsNotTransferred => "annotation.dimension-anchors-not-transferred",
            Self::DrawingStyleNotTransferred => "drawing.style-not-transferred",
            Self::CaptionNotTransferred => "drawing.caption-not-transferred",
            Self::DrawingSheetNotTransferred => "drawing.sheet-not-transferred",
            Self::DrawingTitleBlockNotTransferred => "drawing.title-block-not-transferred",
            Self::DrawingCosmeticEdgeNotTransferred => "drawing.cosmetic-edge-not-transferred",
            Self::DrawingCosmeticVertexNotTransferred => "drawing.cosmetic-vertex-not-transferred",
            Self::DrawingCenterLineNotTransferred => "drawing.center-line-not-transferred",
            Self::DrawingGeometryFormatNotTransferred => "drawing.geometry-format-not-transferred",
            Self::DrawingDimensionKindNotTransferred => "drawing.dimension-kind-not-transferred",
            Self::DrawingDimensionGeometryNotTransferred => {
                "drawing.dimension-geometry-not-transferred"
            }
            Self::DrawingDimensionToleranceNotTransferred => {
                "drawing.dimension-tolerance-not-transferred"
            }
            Self::ArchiveEntryUnreadable => "source.archive-entry-unreadable",
            Self::MeshPlacementNotTransferred => "geometry.mesh-placement-not-transferred",
            Self::MeshPlacementBounded => "geometry.mesh-placement-bounded",
            Self::PersistencePayloadUnresolved => "metadata.persistence-payload-unresolved",
            Self::PersistenceSideEntryUnresolved => "metadata.persistence-side-entry-unresolved",
            Self::PersistenceCountNoncanonical => "metadata.persistence-count-noncanonical",
            Self::FeatureCyclicHistory => "feature.cyclic-history",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::FeatureRevolutionNotTransferred => "feature.revolution-not-transferred",
            Self::SketchNativeGeometry => "sketch.native-geometry",
            Self::SketchNativeConstraint => "sketch.native-constraint",
            Self::SketchExternalReferenceUnresolved => "sketch.external-reference-unresolved",
            Self::SketchExternalCacheUnresolved => "sketch.external-cache-unresolved",
            Self::SketchProfileMatchingBounded => "sketch.profile-matching-bounded",
            Self::AppearanceTopologyColorCountMismatch => {
                "appearance.topology-color-count-mismatch"
            }
            Self::AppearancePrimitiveSizeNotTransferred => {
                "appearance.primitive-size-not-transferred"
            }
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::SourceGuiSchemaUnverified => "source.gui-schema-unverified",
            Self::SourceGuiPropertyKeyBlank => "source.gui-property-key-blank",
            Self::SourceGuiMetadataUnresolved => "source.gui-metadata-unresolved",
            Self::ElementMapMetadataUnresolved => "element-map.metadata-unresolved",
            Self::PcurveNotTransferred => "pcurve.not-transferred",
            Self::PcurveLocationAmbiguous => "pcurve.location-ambiguous",
            Self::TopologyLocationIdentityBounded => "topology.location-identity-bounded",
            Self::ProgramVersionNoncanonical => "metadata.program-version-noncanonical",
            Self::FileVersionNoncanonical => "metadata.file-version-noncanonical",
            Self::FileVersionUnverified => "metadata.file-version-unverified",
            Self::ElementMapVersionUnresolved => "element-map.version-unresolved",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::MeshPlacementNotTransferred
            | Self::MeshPlacementBounded
            | Self::FeatureCyclicHistory
            | Self::FeatureNativeKindRetained
            | Self::FeatureRevolutionNotTransferred
            | Self::SketchNativeGeometry
            | Self::SketchNativeConstraint
            | Self::PcurveLocationAmbiguous
            | Self::TopologyLocationIdentityBounded => Severity::Blocking,
            Self::DrawingViewAxisNotTransferred
            | Self::TemplateNotTransferred
            | Self::ViewGeometryNotStored
            | Self::ViewGeometryNotTransferred
            | Self::DimensionAnchorsNotTransferred
            | Self::DrawingStyleNotTransferred
            | Self::CaptionNotTransferred
            | Self::DrawingSheetNotTransferred
            | Self::DrawingTitleBlockNotTransferred
            | Self::DrawingCosmeticEdgeNotTransferred
            | Self::DrawingCosmeticVertexNotTransferred
            | Self::DrawingCenterLineNotTransferred
            | Self::DrawingGeometryFormatNotTransferred
            | Self::DrawingDimensionKindNotTransferred
            | Self::DrawingDimensionGeometryNotTransferred
            | Self::DrawingDimensionToleranceNotTransferred
            | Self::SketchExternalCacheUnresolved
            | Self::SketchExternalReferenceUnresolved
            | Self::SketchProfileMatchingBounded
            | Self::ArchiveEntryUnreadable
            | Self::PersistencePayloadUnresolved
            | Self::PersistenceSideEntryUnresolved
            | Self::PersistenceCountNoncanonical
            | Self::AppearanceTopologyColorCountMismatch
            | Self::AppearancePrimitiveSizeNotTransferred
            | Self::SourceDialectUnverified
            | Self::SourceGuiSchemaUnverified
            | Self::SourceGuiPropertyKeyBlank
            | Self::SourceGuiMetadataUnresolved
            | Self::ElementMapMetadataUnresolved
            | Self::PcurveNotTransferred
            | Self::ProgramVersionNoncanonical
            | Self::FileVersionNoncanonical
            | Self::FileVersionUnverified
            | Self::ElementMapVersionUnresolved => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::MeshPlacementNotTransferred | Self::MeshPlacementBounded => {
                LossTaxonomy::GeometryNotTransferred
            }
            Self::DrawingViewAxisNotTransferred
            | Self::TemplateNotTransferred
            | Self::ViewGeometryNotStored
            | Self::ViewGeometryNotTransferred
            | Self::DimensionAnchorsNotTransferred
            | Self::DrawingStyleNotTransferred
            | Self::CaptionNotTransferred
            | Self::DrawingSheetNotTransferred
            | Self::DrawingTitleBlockNotTransferred
            | Self::DrawingCosmeticEdgeNotTransferred
            | Self::DrawingCosmeticVertexNotTransferred
            | Self::DrawingCenterLineNotTransferred
            | Self::DrawingGeometryFormatNotTransferred
            | Self::DrawingDimensionKindNotTransferred
            | Self::DrawingDimensionGeometryNotTransferred
            | Self::DrawingDimensionToleranceNotTransferred
            | Self::ArchiveEntryUnreadable
            | Self::PersistencePayloadUnresolved
            | Self::PersistenceSideEntryUnresolved => LossTaxonomy::RecordNotTyped,
            Self::FeatureCyclicHistory
            | Self::FeatureNativeKindRetained
            | Self::FeatureRevolutionNotTransferred => LossTaxonomy::FeatureHistoryRetained,
            Self::SketchExternalCacheUnresolved
            | Self::SketchExternalReferenceUnresolved
            | Self::SketchNativeGeometry
            | Self::SketchNativeConstraint
            | Self::SketchProfileMatchingBounded
            | Self::SourceGuiPropertyKeyBlank
            | Self::SourceGuiMetadataUnresolved
            | Self::ElementMapMetadataUnresolved
            | Self::PcurveNotTransferred
            | Self::PcurveLocationAmbiguous
            | Self::TopologyLocationIdentityBounded => LossTaxonomy::RecordNotTyped,
            Self::AppearanceTopologyColorCountMismatch
            | Self::AppearancePrimitiveSizeNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::PersistenceCountNoncanonical
            | Self::ProgramVersionNoncanonical
            | Self::FileVersionNoncanonical
            | Self::FileVersionUnverified => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ElementMapVersionUnresolved => LossTaxonomy::RecordNotTyped,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::SourceGuiSchemaUnverified => LossTaxonomy::SourceDialectUnverified,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    fn kind(self) -> LossKind {
        let strict_floor = match self {
            Self::SourceGuiSchemaUnverified => None,
            other => other.shared_taxonomy().strict_floor(),
        };
        NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy())
            .with_strict_floor(strict_floor)
            .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `fcstd/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
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
                "drawing.template-not-transferred",
                "drawing.view-axis-not-transferred",
                "drawing.view-geometry-not-stored",
                "drawing.view-geometry-not-transferred",
                "annotation.dimension-anchors-not-transferred",
                "drawing.style-not-transferred",
                "drawing.caption-not-transferred",
                "drawing.sheet-not-transferred",
                "drawing.title-block-not-transferred",
                "drawing.cosmetic-edge-not-transferred",
                "drawing.cosmetic-vertex-not-transferred",
                "drawing.center-line-not-transferred",
                "drawing.geometry-format-not-transferred",
                "drawing.dimension-kind-not-transferred",
                "drawing.dimension-geometry-not-transferred",
                "drawing.dimension-tolerance-not-transferred",
                "source.archive-entry-unreadable",
                "geometry.mesh-placement-not-transferred",
                "geometry.mesh-placement-bounded",
                "metadata.persistence-payload-unresolved",
                "metadata.persistence-side-entry-unresolved",
                "metadata.persistence-count-noncanonical",
                "feature.cyclic-history",
                "feature.native-kind-retained",
                "sketch.native-geometry",
                "sketch.native-constraint",
                "sketch.external-reference-unresolved",
                "sketch.external-cache-unresolved",
                "sketch.profile-matching-bounded",
                "appearance.topology-color-count-mismatch",
                "appearance.primitive-size-not-transferred",
                "source.dialect-unverified",
                "source.gui-schema-unverified",
                "source.gui-property-key-blank",
                "source.gui-metadata-unresolved",
                "element-map.metadata-unresolved",
                "pcurve.not-transferred",
                "pcurve.location-ambiguous",
                "topology.location-identity-bounded",
                "metadata.program-version-noncanonical",
                "metadata.file-version-noncanonical",
                "metadata.file-version-unverified",
                "element-map.version-unresolved",
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

    #[test]
    fn gui_schema_recovery_does_not_trigger_document_dialect_strictness() {
        assert_eq!(
            FreecadLossCode::SourceGuiSchemaUnverified
                .kind()
                .strict_floor(),
            None
        );
        assert_eq!(
            FreecadLossCode::SourceDialectUnverified
                .kind()
                .strict_floor(),
            Some(cadmpeg_ir::report::Severity::Warning)
        );
    }
}
