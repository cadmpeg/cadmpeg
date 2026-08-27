// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Inventor IPT/IAM decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`InventorLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`InventorLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `inventor` namespace.
//!
//! [`InventorLossCode::shared_taxonomy`] is an exhaustive match with no
//! fall-through arm. A default arm would silently assign a category to a code
//! added later, and the categories this codec spans have no honest common
//! default.

use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

/// A stable, machine-readable identifier for one Inventor transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`InventorLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InventorLossCode {
    /// Container-only decode was requested; no entity transfer ran.
    ContainerOnlyDecode,
    /// The active kernel carrier was not transferred into neutral geometry.
    GeometryKernelCarrierNotTransferred,
    /// Faces use procedural surfaces without a decoded carrier.
    GeometryProceduralSurfaceNotTransferred,
    /// Structurally paired `RSe` segments have no record semantics.
    RseSegmentPairUntyped,
    /// `RSe` metadata streams are malformed or outside the implemented envelope.
    RseMetadataStreamMalformed,
    /// `RSe` bulk streams have invalid envelope or zlib framing.
    RseBulkStreamMalformed,
    /// Typed assembly records are malformed or outside the implemented branch.
    AssemblyRecordMalformed,
    /// Typed presentation records are malformed or outside the implemented branch.
    PresentationRecordMalformed,
    /// Typed design records are malformed or outside the implemented branch.
    DesignRecordMalformed,
    /// Typed sketch records could not be parsed exactly.
    SketchRecordMalformed,
    /// Typed feature records could not be parsed exactly.
    FeatureRecordMalformed,
    /// Typed feature records have an operation graph that is not closed.
    FeatureOperationGraphOpen,
    /// Inventor operations retain native result-body identity with unresolved state.
    FeatureStateUnresolved,
    /// Parameter records have a unit or expression graph that is not closed.
    ParameterGraphOpen,
    /// Sketch, entity, or constraint records have a neutral graph that is not closed.
    SketchGraphOpen,
    /// `RSe` contains unpaired metadata or bulk streams.
    RseStreamUnpaired,
    /// OLE property-set streams are malformed.
    PropertySetStreamMalformed,
    /// Property values have no neutral metadata mapping.
    MetadataPropertyUnmapped,
    /// The Protein asset catalog could not be decoded.
    ProteinCatalogUndecodable,
    /// Malformed Protein asset records were rejected.
    ProteinAssetRejected,
    /// The Protein package contains no decoded appearance assets.
    ProteinAppearanceAbsent,
    /// `PmApp` document-default appearance assignments did not resolve.
    AppearanceDefaultUnresolved,
    /// Protein catalog asset GUIDs collide; ambiguous texture joins were refused.
    ProteinGuidAmbiguous,
    /// The Inventor Protein stream is malformed.
    ProteinStreamMalformed,
    /// `PmGraphics` face appearance overrides did not resolve.
    AppearanceFaceOverrideUnresolved,
    /// The `UFRxDoc` external-reference table is malformed.
    UfrxTableMalformed,
    /// An unsupported `UFRxDoc` schema was retained on an assembly document.
    UfrxSchemaUnsupportedAssembly,
    /// An unsupported `UFRxDoc` schema was retained without a typed transfer.
    UfrxSchemaUnsupported,
    /// External component references remain unresolved.
    AssemblyComponentExternal,
    /// Assembly occurrence placements could not be transferred.
    AssemblyPlacementNotTransferred,
    /// The document was read with a grammar its own declarations do not select.
    SourceDialectUnverified,
    /// The active kernel carrier used an unverified Spatial ACIS grammar band.
    KernelDialectUnverified,
}

impl InventorLossCode {
    /// Every code, in declaration order.
    #[allow(dead_code)] // Catalog for crate tests and harness oracles.
    pub const ALL: &'static [InventorLossCode] = &[
        Self::ContainerOnlyDecode,
        Self::GeometryKernelCarrierNotTransferred,
        Self::GeometryProceduralSurfaceNotTransferred,
        Self::RseSegmentPairUntyped,
        Self::RseMetadataStreamMalformed,
        Self::RseBulkStreamMalformed,
        Self::AssemblyRecordMalformed,
        Self::PresentationRecordMalformed,
        Self::DesignRecordMalformed,
        Self::SketchRecordMalformed,
        Self::FeatureRecordMalformed,
        Self::FeatureOperationGraphOpen,
        Self::FeatureStateUnresolved,
        Self::ParameterGraphOpen,
        Self::SketchGraphOpen,
        Self::RseStreamUnpaired,
        Self::PropertySetStreamMalformed,
        Self::MetadataPropertyUnmapped,
        Self::ProteinCatalogUndecodable,
        Self::ProteinAssetRejected,
        Self::ProteinAppearanceAbsent,
        Self::AppearanceDefaultUnresolved,
        Self::ProteinGuidAmbiguous,
        Self::ProteinStreamMalformed,
        Self::AppearanceFaceOverrideUnresolved,
        Self::UfrxTableMalformed,
        Self::UfrxSchemaUnsupportedAssembly,
        Self::UfrxSchemaUnsupported,
        Self::AssemblyComponentExternal,
        Self::AssemblyPlacementNotTransferred,
        Self::SourceDialectUnverified,
        Self::KernelDialectUnverified,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ContainerOnlyDecode => "container.only-decode",
            Self::GeometryKernelCarrierNotTransferred => "geometry.kernel-carrier-not-transferred",
            Self::GeometryProceduralSurfaceNotTransferred => {
                "geometry.procedural-surface-not-transferred"
            }
            Self::RseSegmentPairUntyped => "rse.segment-pair-untyped",
            Self::RseMetadataStreamMalformed => "rse.metadata-stream-malformed",
            Self::RseBulkStreamMalformed => "rse.bulk-stream-malformed",
            Self::AssemblyRecordMalformed => "assembly.record-malformed",
            Self::PresentationRecordMalformed => "presentation.record-malformed",
            Self::DesignRecordMalformed => "design.record-malformed",
            Self::SketchRecordMalformed => "sketch.record-malformed",
            Self::FeatureRecordMalformed => "feature.record-malformed",
            Self::FeatureOperationGraphOpen => "feature.operation-graph-open",
            Self::FeatureStateUnresolved => "feature.state-unresolved",
            Self::ParameterGraphOpen => "parameter.graph-open",
            Self::SketchGraphOpen => "sketch.graph-open",
            Self::RseStreamUnpaired => "rse.stream-unpaired",
            Self::PropertySetStreamMalformed => "property-set.stream-malformed",
            Self::MetadataPropertyUnmapped => "metadata.property-unmapped",
            Self::ProteinCatalogUndecodable => "protein.catalog-undecodable",
            Self::ProteinAssetRejected => "protein.asset-rejected",
            Self::ProteinAppearanceAbsent => "protein.appearance-absent",
            Self::AppearanceDefaultUnresolved => "appearance.default-unresolved",
            Self::ProteinGuidAmbiguous => "protein.guid-ambiguous",
            Self::ProteinStreamMalformed => "protein.stream-malformed",
            Self::AppearanceFaceOverrideUnresolved => "appearance.face-override-unresolved",
            Self::UfrxTableMalformed => "ufrx.table-malformed",
            Self::UfrxSchemaUnsupportedAssembly => "ufrx.schema-unsupported-assembly",
            Self::UfrxSchemaUnsupported => "ufrx.schema-unsupported",
            Self::AssemblyComponentExternal => "assembly.component-external",
            Self::AssemblyPlacementNotTransferred => "assembly.placement-not-transferred",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::ContainerOnlyDecode => Severity::Info,
            Self::GeometryKernelCarrierNotTransferred => Severity::Blocking,
            _ => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerOnlyDecode => LossTaxonomy::ContainerOnly,
            Self::GeometryKernelCarrierNotTransferred
            | Self::GeometryProceduralSurfaceNotTransferred => LossTaxonomy::GeometryNotTransferred,
            Self::RseSegmentPairUntyped | Self::UfrxSchemaUnsupported => {
                LossTaxonomy::RecordNotTyped
            }
            Self::RseMetadataStreamMalformed
            | Self::RseBulkStreamMalformed
            | Self::AssemblyRecordMalformed
            | Self::PresentationRecordMalformed
            | Self::DesignRecordMalformed
            | Self::SketchRecordMalformed
            | Self::FeatureRecordMalformed
            | Self::RseStreamUnpaired
            | Self::PropertySetStreamMalformed
            | Self::ProteinStreamMalformed
            | Self::UfrxTableMalformed => LossTaxonomy::DecodeDiagnostic,
            Self::FeatureOperationGraphOpen
            | Self::FeatureStateUnresolved
            | Self::SketchGraphOpen => LossTaxonomy::FeatureHistoryRetained,
            Self::ParameterGraphOpen => LossTaxonomy::ParametricRecordOmitted,
            Self::MetadataPropertyUnmapped => LossTaxonomy::MetadataNotTransferred,
            Self::ProteinCatalogUndecodable
            | Self::ProteinAssetRejected
            | Self::ProteinAppearanceAbsent
            | Self::AppearanceDefaultUnresolved
            | Self::ProteinGuidAmbiguous
            | Self::AppearanceFaceOverrideUnresolved => LossTaxonomy::MaterialNotTransferred,
            Self::UfrxSchemaUnsupportedAssembly | Self::AssemblyComponentExternal => {
                LossTaxonomy::AssemblyComponentsExternal
            }
            Self::AssemblyPlacementNotTransferred => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced("inventor", self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `inventor/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::InventorLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = InventorLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.only-decode",
                "geometry.kernel-carrier-not-transferred",
                "geometry.procedural-surface-not-transferred",
                "rse.segment-pair-untyped",
                "rse.metadata-stream-malformed",
                "rse.bulk-stream-malformed",
                "assembly.record-malformed",
                "presentation.record-malformed",
                "design.record-malformed",
                "sketch.record-malformed",
                "feature.record-malformed",
                "feature.operation-graph-open",
                "feature.state-unresolved",
                "parameter.graph-open",
                "sketch.graph-open",
                "rse.stream-unpaired",
                "property-set.stream-malformed",
                "metadata.property-unmapped",
                "protein.catalog-undecodable",
                "protein.asset-rejected",
                "protein.appearance-absent",
                "appearance.default-unresolved",
                "protein.guid-ambiguous",
                "protein.stream-malformed",
                "appearance.face-override-unresolved",
                "ufrx.table-malformed",
                "ufrx.schema-unsupported-assembly",
                "ufrx.schema-unsupported",
                "assembly.component-external",
                "assembly.placement-not-transferred",
                "source.dialect-unverified",
                "source.kernel-dialect-unverified",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in InventorLossCode::ALL {
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
        for code in InventorLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert_eq!(note.code.namespace(), "inventor");
            assert_eq!(note.code.local_code(), code.code());
            assert!(note.provenance.is_none());
        }
    }
}
